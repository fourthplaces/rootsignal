export function eventHue(name: string): number {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return ((hash % 360) + 360) % 360;
}

export function eventBg(name: string): string {
  return `hsla(${eventHue(name)}, 70%, 50%, 0.2)`;
}

export function eventBorder(name: string): string {
  return `hsl(${eventHue(name)}, 70%, 50%)`;
}

export function eventTextColor(name: string): string {
  return `hsl(${eventHue(name)}, 70%, 65%)`;
}
