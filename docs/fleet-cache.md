# Fleet View Cache

CC retains one complete, grouped fleet view in memory. List requests still apply
their own search, visibility, presence, agent filters and result limit. Neither
the database nor the response format changes.

## Inputs And Invalidation

The cache key combines the store's fleet revision and connected-agent IDs.
Successful snapshot writes, saved-device create/rename/forget/merge, identifier
edits, agent enrollment/revocation/nickname edits, and device-retention cleanup
advance the revision. Token-only, audit and alert writes do not invalidate it.
Connection changes are detected from the live session map on each read.

The next reader rebuilds a changed view. Concurrent readers wait for that build
and reuse it. The helper checks the key again after building: if inputs changed
while database reads/grouping were in flight, it discards the result and retries.
Build errors are returned, not cached; cancelling a reader releases the lock.
The previous view is not served as a fallback after a rebuild error.
After three invalidated builds, the reader stops and releases the lock. Fleet
list/wake requests return HTTP 503 with code `fleet_busy`; clients can retry
later. No stale view or command is used as a fallback. This bounds the number
of rebuilds per reader, not database execution time or total concurrent requests.

Wake searches the complete cached view by device key, without list visibility
filters or the list API's 1,000-row limit. It borrows the view and clones only the
selected route. Command dispatch still checks the current session;
a cached route cannot grant access to a disconnected agent.

`GET /api/v1/control/fleet/devices/{device_key}` returns one exact row from the
complete view, or `404 fleet_device_not_found`. Details do not inherit list
filters or limits. After Remember/Attach/Merge, the UI follows the saved ID
returned by the mutation, not matching MAC/IP heuristics. It also refreshes the
full saved identity independently of list results; Forget still closes details.

## External Writes

The revision belongs to the running CC process. If another process directly
modifies its SQLite database, run the existing CC reload command (or send
SIGHUP) to invalidate the view, or restart CC. The cache does not watch files
or poll SQLite for external changes. Normal HTTP/WS store writes invalidate
automatically, and there is no time-based cache expiry.

## Shared Helper And Tracing

`runtime::view_cache::ViewCache` owns synchronization and stale-build rejection.
Fleet code supplies the key and builder. It is intentionally CC-local; alerts
keep their existing timer-driven evaluator rather than being forced into this
lazy cache.

`fleet_load` spans remain on requests. A cache miss adds `fleet_rebuild`, with
database loads and `fleet_group_devices` nested below it. On a cache hit, list
requests clone/filter the view; wake requests look up their device in the borrowed
view. Neither path repeats the database reads or grouping.
