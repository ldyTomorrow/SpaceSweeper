export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";

  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / 1024 ** index;
  const digits = index === 0 || value >= 100 ? 0 : 1;

  return `${value.toFixed(digits)} ${units[index]}`;
}

export function formatDate(ms: number | null): string {
  if (!ms) return "—";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(new Date(ms));
}

export function formatCount(value: number): string {
  return new Intl.NumberFormat("zh-CN").format(value);
}

export function displayHomePath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export function formatAge(days: number): string {
  if (days <= 0) return "今天";
  if (days < 30) return `${days} 天`;
  if (days < 365) return `${Math.max(1, Math.round(days / 30))} 个月`;
  const years = days / 365;
  return `${years < 10 ? years.toFixed(1) : Math.round(years)} 年`;
}

export function toErrorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return JSON.stringify(error);
}
