/** Format Unix seconds as elapsed time for recent events and local time for older ones. */
export function formatRecentTime(value: number | null): string {
  if (!value) return "-";
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - value);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return new Date(value * 1000).toLocaleString();
}
