// Lines of the public-domain poems in data/echo/public, for "random line".
// (The private corpus is never bundled.)
const files = import.meta.glob('../../../../data/echo/public/**/*.txt', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

export interface PoemLine {
  poem: string;
  line: string;
}

export const LINES: PoemLine[] = Object.entries(files).flatMap(([path, text]) => {
  const header = text.split('\n\n')[0] ?? '';
  const title = /^title:\s*(.+)$/m.exec(header)?.[1] ?? path;
  const author = /^author:\s*(.+)$/m.exec(header)?.[1] ?? '';
  const body = text.slice(header.length);
  return body
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l.split(/\s+/).length >= 3)
    .map((line) => ({ poem: author ? `${author}, “${title}”` : title, line }));
});
