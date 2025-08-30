# taking back my code AHHHH

this is going to be lengthy. ci is spotless but idk how it works cuz powershell. this is good but i cant afford VPN on the router; we will need to do some bullshit.

## overview

we need a:

- small ahh bin in the router.
  - forwards a shell and some functions.
  - may be shell calling fns maybe some unsafe c. i cant unsafe c.
- big ahh bin in the vps.
  - connect to the small ahh bin thru the wide ass internet.
  - forwards some APIs.
- static site
  - now need no hosts; apis can be called anywhere anyway.
  - do offer these in the big ahh bins tho lets not miss out some shit.
- automation
  - gitea
    - on the vps or the uh idk.
    - ABSOLUTELY can auto build main:latest every day / other day.
    - and releases.
  - router
    - a script to pull the tar. from actions artifacts or from releases. (lua or shell yo)
    - a script for the bin and the init.d scripts for these two.
  - vps
    - a systemd service (ima use ai for ts ngl)
    - an auto updater. maybe we overkill and dnf. maybe go easy and git clone + cargo install. maybe we pull from release or nightly. idk

i think idk

## router

### current

everything.

we have some functions that call ip neigh a bajillion times.
we have some other functions that read the same file/folder in /sys or sum shit.
we have a function that broadcast to a set ahh address.

we have some routes; and some data structures acting as schema for them ahh. we dont do posts with json but gets with query strings for some ahh reasons.

these are carryover from the early days.

also from the early days are the js.

static was 100% ai with 0 intervention.

we have a search bar of sort that hooks through a smart endpoint that searches for everything.

we have some bullshit. and two buttons to check and wake. that pings `/api/smart/{query}` and `/wake?name=query`. why.
we have the result of the wake and or status and the errors in the first text box.

the list of cliets with names connected to the router the second box.
thats cool.

### changes?

idk changes.
