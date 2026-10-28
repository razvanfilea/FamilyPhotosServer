// Builds assets/icons/sprite.svg from the Material Symbols Rounded icons listed in scripts/icons.txt.
// Every icon gets two symbols: `#name` (outlined) and `#name-fill` (filled).
import { readFileSync, writeFileSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const iconsDir = join(root, 'node_modules/@material-symbols/svg-400/rounded');
const outDir = join(root, 'assets/icons');

const names = readFileSync(join(root, 'scripts/icons.txt'), 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'));

const duplicates = names.filter((name, i) => names.indexOf(name) !== i);
if (duplicates.length > 0) {
    throw new Error(`Duplicate icons in icons.txt: ${duplicates.join(', ')}`);
}

function symbol(id, file) {
    const svg = readFileSync(join(iconsDir, file), 'utf8');
    const viewBox = svg.match(/viewBox="([^"]+)"/)[1];
    const body = svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
    return `<symbol id="${id}" viewBox="${viewBox}">${body}</symbol>`;
}

const symbols = names.flatMap((name) => [
    symbol(name, `${name}.svg`),
    symbol(`${name}-fill`, `${name}-fill.svg`),
]);

writeFileSync(
    join(outDir, 'sprite.svg'),
    `<svg xmlns="http://www.w3.org/2000/svg">\n${symbols.join('\n')}\n</svg>\n`,
);
copyFileSync(join(root, 'node_modules/@material-symbols/svg-400/LICENSE'), join(outDir, 'LICENSE'));

console.log(`Wrote ${symbols.length} symbols for ${names.length} icons to assets/icons/sprite.svg`);
