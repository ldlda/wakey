import { Button, buttonVariants } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { DeviceRow, SortDir } from "@/pages/devices/types";
import {
  chooseWakeTarget,
  nextSort,
  summarize,
  type SortKey,
} from "@/pages/devices/utils";

type Props = {
  rows: DeviceRow[];
  selectedIds: string[];
  sort: { key: SortKey; dir: SortDir };
  allVisibleSelected: boolean;
  wakeBusyId: string;
  selectedAgentId: string;
  bulkWakeBusy: boolean;
  onToggleAllVisible: (checked: boolean) => void;
  onToggleRow: (id: string, checked: boolean) => void;
  onSortChange: (next: { key: SortKey; dir: SortDir }) => void;
  onWakeDevice: (device: DeviceRow) => void;
  onCopyValue: (label: string, value: string) => void;
  onOpenDetails: (device: DeviceRow) => void;
};

function sortArrow(active: boolean, dir: SortDir) {
  if (!active) return "";
  return dir === "asc" ? "▲" : "▼";
}

export function DeviceInventoryTable({
  rows,
  selectedIds,
  sort,
  allVisibleSelected,
  wakeBusyId,
  selectedAgentId,
  bulkWakeBusy,
  onToggleAllVisible,
  onToggleRow,
  onSortChange,
  onWakeDevice,
  onCopyValue,
  onOpenDetails,
}: Props) {
  function shouldIgnoreRowClick(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return Boolean(target.closest("button, input, a, [role='menuitem']"));
  }

  return (
    <div className="device-list grid gap-2">
      <div className="device-row device-header rounded-md border bg-muted/60 px-3 py-2 text-sm">
        <span className="device-cell device-select">
          <input
            type="checkbox"
            checked={allVisibleSelected}
            onChange={(e) => onToggleAllVisible(e.target.checked)}
            disabled={!rows.length}
            aria-label="Select all visible devices"
          />
        </span>
        <span
          className="sortable-col device-cell"
          onClick={() => onSortChange(nextSort(sort, "name"))}
        >
          Name {sortArrow(sort.key === "name", sort.dir)}
        </span>
        <span
          className="sortable-col device-cell"
          onClick={() => onSortChange(nextSort(sort, "ip"))}
        >
          IP {sortArrow(sort.key === "ip", sort.dir)}
        </span>
        <span
          className="sortable-col device-cell"
          onClick={() => onSortChange(nextSort(sort, "mac"))}
        >
          MAC {sortArrow(sort.key === "mac", sort.dir)}
        </span>
        <span
          className="sortable-col device-cell"
          onClick={() => onSortChange(nextSort(sort, "presence"))}
        >
          Presence {sortArrow(sort.key === "presence", sort.dir)}
        </span>
        <span className="device-cell device-action">Actions</span>
      </div>

      {rows.map((row) => (
        <div
          className="device-row cursor-pointer rounded-md border bg-card px-3 py-2 text-sm transition-colors hover:bg-accent/30"
          key={row.id}
          role="button"
          tabIndex={0}
          onClick={(event) => {
            if (shouldIgnoreRowClick(event.target)) return;
            onOpenDetails(row);
          }}
          onKeyDown={(event) => {
            if (shouldIgnoreRowClick(event.target)) return;
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onOpenDetails(row);
            }
          }}
        >
          <span className="device-cell device-select" data-label="Pick">
            <input
              type="checkbox"
              checked={selectedIds.includes(row.id)}
              onChange={(e) => onToggleRow(row.id, e.target.checked)}
              aria-label={`Select ${row.name}`}
            />
          </span>
          <span className="device-cell" data-label="Name" title={row.name}>
            {row.name}
          </span>
          <span
            className="device-cell text-muted-foreground"
            data-label="IP"
            title={row.ips.join(", ") || "-"}
          >
            {summarize(row.ips)}
          </span>
          <span
            className="device-cell text-muted-foreground"
            data-label="MAC"
            title={row.macs.join(", ") || "-"}
          >
            {summarize(row.macs)}
          </span>
          <span className="device-cell" data-label="Presence">
            <Badge variant="outline">{row.presence}</Badge>
          </span>
          <span className="device-cell device-action" data-label="">
            <Button
              onClick={() => onWakeDevice(row)}
              disabled={
                wakeBusyId === row.id || !selectedAgentId || bulkWakeBusy
              }
              size="sm"
              className="device-btn"
            >
              {wakeBusyId === row.id ? "Waking..." : "Wake"}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger
                className={cn(
                  buttonVariants({ variant: "outline", size: "sm" }),
                  "device-copy-trigger",
                )}
              >
                Copy
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-32">
                <DropdownMenuItem
                  onClick={() => onCopyValue("target", chooseWakeTarget(row))}
                >
                  Copy target
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => onCopyValue("name", row.name)}>
                  Copy name
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => onCopyValue("ip", row.ips[0] || "")}
                >
                  Copy IP
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => onCopyValue("mac", row.macs[0] || "")}
                >
                  Copy MAC
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </span>
        </div>
      ))}

      {!rows.length && (
        <div className="px-1 py-2 text-sm text-muted-foreground">
          No devices found
        </div>
      )}
    </div>
  );
}
