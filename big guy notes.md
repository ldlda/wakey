# lda rambling

currently the collection of fleetwakeroutes doesnt make much sense.

it goes deep. Read code, then this again.

## big fleet shows everything.

all the IPs, offline or not. I at least want to hide the offline/unknown IPs into a drawer somewhere. its too cluttered.

i do need offline IPs, because, a failed / dhcp-rotated IP may be of use. but what use? copying only. You cant even send shit to it, its FAILED.

so not all the IPs should be immediately invisible.

## big fleet DOESNT show everything

yea showing all the offline IP but only ONE online ip per mac for wake routes.

BRING This BACK. i need ALL the routes. Color coded (already has that with disabled wake button)

## big collect collects everything

at wakey, collection makes no sense; a device saves all IPs without saying which of them is not real.

oh you do have to search again for the observations because they are unknown ahh hints,

## logics are misleading

also IPs without macs are unknown, (should be OFFLINE? ips without macs? what do you think it is)
while macs without ips are offline (pulled straight from the Big Mac Name cache)

what is this ahh logic

## struct names are misleading

Big Device over here, collected (fresh AND old) From the router ITSELF. Is BY MAC.

but the Device struct was designed to take multiple MACs, Why. This is very misleading.

The migration was from Observations, which is a string fest, to Device, which

1. give up the very cool observations based design, which fundamentally is just tracking identifing pairs of (ip, mac, maybe hostname)
2. doesnt solve the confusing ahh mac-keyed Devices. Why is the deviceid optional? What!

## big code needs a rewrite

i think we need to have a very clear model. And such model is ALREADY PRESENT! fleet wake route MIGHT be the coolest thing ever, solves everything. 

Should type instead of free string construction. For example. alot of the keys are currently freehanded. You need a struct with Display or ToString or a dedicated method.
hopefully those keys can have a helper contructor. HOPEFULLY NOONE PULLS INFO OUT OF THE STRING BY SLICING IT.

If you want to keep just device.macs, device.ips, you can, we need to collect the Wake routes! which goes deep, 

but is reasonable, because there are two sources that give us identifiers, neighbors and observations. We flatten this to device, at the router its keyed by mac or failed ip.
At the fleet idfk, i dont control that part of the code; its all AI.

So, if we can collect wake targets directly from device. Saves a whole plane of problems. suddenly wakey-cc is like 6 million times slimmer, because this is a wakey or wakey-core problem.