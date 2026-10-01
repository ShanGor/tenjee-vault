const DAY_MS = 86_400_000;

export function parseDate(value: string): Date {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) throw new Error(`Invalid date: ${value}`);
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  if (formatDate(date) !== value) throw new Error(`Invalid date: ${value}`);
  return date;
}

export function formatDate(date: Date): string {
  const year = date.getFullYear().toString().padStart(4, "0");
  const month = (date.getMonth() + 1).toString().padStart(2, "0");
  const day = date.getDate().toString().padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function addDays(value: string, amount: number): string {
  const date = parseDate(value);
  date.setDate(date.getDate() + amount);
  return formatDate(date);
}

export function startOfWeek(value: string): string {
  const date = parseDate(value);
  const mondayOffset = (date.getDay() + 6) % 7;
  return addDays(value, -mondayOffset);
}

export function weekDates(value: string): string[] {
  const start = startOfWeek(value);
  return Array.from({ length: 7 }, (_, index) => addDays(start, index));
}

/** Complete Monday–Sunday rows containing every date in the anchor month. */
export function monthGrid(value: string): string[] {
  const anchor = parseDate(value);
  const first = formatDate(new Date(anchor.getFullYear(), anchor.getMonth(), 1));
  const last = formatDate(new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0));
  const start = startOfWeek(first);
  const end = addDays(startOfWeek(last), 6);
  const count = Math.round((parseDate(end).getTime() - parseDate(start).getTime()) / DAY_MS) + 1;
  return Array.from({ length: count }, (_, index) => addDays(start, index));
}
