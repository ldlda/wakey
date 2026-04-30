import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";

export function PresenceBadge({ presence }: { presence: string }) {
  return (
    <Badge variant="outline" className={`presence-badge--${presence}`}>
      <span className={`presence-dot presence-dot--${presence}`} aria-hidden />
      {presence.replace("_", " ")}
    </Badge>
  );
}

export function MobileLabel({ label }: { label: string }) {
  return (
    <span className="mb-1 block text-xs font-medium text-muted-foreground xl:hidden">
      {label}
    </span>
  );
}

export function FilterSelect({
  label,
  value,
  values,
  onChange,
}: {
  label: string;
  value: string;
  values: readonly string[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-1 text-sm text-muted-foreground">
      <span>{label}</span>
      <Select value={value} onValueChange={(next) => next && onChange(next)}>
        <SelectTrigger>
          <span>{value.replace("_", " ")}</span>
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {values.map((item) => (
            <SelectItem key={item} value={item}>
              {item.replace("_", " ")}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  );
}

export function DetailBlock({
  label,
  values,
  onCopy,
}: {
  label: string;
  values: string[];
  onCopy: (label: string, value: string) => void;
}) {
  return (
    <div className="rounded-md border bg-muted/30 p-3">
      <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
        {label}
      </div>
      <div className="grid gap-1">
        {values.length ? (
          values.map((value) => (
            <button
              key={value}
              type="button"
              className="flex min-w-0 items-center justify-between gap-2 rounded px-1 py-0.5 text-left hover:bg-accent"
              onClick={() => onCopy(label, value)}
            >
              <span className="min-w-0 truncate">{value}</span>
              <CopyIcon />
            </button>
          ))
        ) : (
          <span className="text-sm text-muted-foreground">-</span>
        )}
      </div>
    </div>
  );
}

function CopyIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className="size-3.5 shrink-0 text-muted-foreground"
      aria-hidden
    >
      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </svg>
  );
}
