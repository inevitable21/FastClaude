/// Sentinel value for "don't pass this flag" in dropdowns. shadcn/Radix
/// Select disallows the empty string as a SelectItem value, so the UI uses
/// this token and we map it back to "" before sending to the backend.
export const UNSET = "__unset__";

export const EFFORT_OPTIONS = ["low", "medium", "high", "xhigh", "max"] as const;

export const PERMISSION_MODE_OPTIONS = [
  "acceptEdits",
  "auto",
  "bypassPermissions",
  "default",
  "dontAsk",
  "plan",
] as const;

export function fromUnset(v: string): string {
  return v === UNSET ? "" : v;
}

export function toUnset(v: string): string {
  return v === "" ? UNSET : v;
}
