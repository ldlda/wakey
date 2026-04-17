import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import type { DeviceRow } from "@/pages/devices/types";

type Props = {
  device: DeviceRow | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

function summarizeAll(values: string[]): string {
  if (!values.length) return "-";
  return values.join(", ");
}

export function DeviceDetailsDialog({ device, open, onOpenChange }: Props) {
  if (!device) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl p-0">
        <div className="grid gap-4 p-4 text-sm">
          <DialogHeader className="pr-8">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <DialogTitle className="truncate">{device.name}</DialogTitle>
                <DialogDescription>Device details</DialogDescription>
              </div>
              <Badge variant="outline">{device.presence}</Badge>
            </div>
          </DialogHeader>

          <div>
            <p className="mb-1 font-medium">IP addresses</p>
            <p className="text-muted-foreground">{summarizeAll(device.ips)}</p>
          </div>
          <Separator />
          <div>
            <p className="mb-1 font-medium">MAC addresses</p>
            <p className="text-muted-foreground">{summarizeAll(device.macs)}</p>
          </div>
          <Separator />
          <div>
            <p className="mb-1 font-medium">Interfaces</p>
            <p className="text-muted-foreground">
              {summarizeAll(device.interfaces)}
            </p>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
