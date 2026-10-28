import { readRunJson } from './run';

export interface SeedPhoto {
    id: number;
    /** `<owner or "public">/[<folder>/]<file name>`, like the path under STORAGE_PATH */
    path: string;
    name: string;
    userId: string | null;
    folderId: number | null;
    /** Unix seconds */
    createdAt: number;
    trashed: boolean;
}

export interface Seed {
    /** Every photo, by path */
    photos: Record<string, SeedPhoto>;
    /** Folder ids by name: "Holiday", "Private" (alice), "Family Trip" (public) */
    folders: Record<string, number>;
    /** Paths of the photos alice marked as favourite */
    aliceFavorites: string[];
    /** Paths of the photos alice moved to the trash */
    trashed: string[];
}

let seed: Seed | undefined;

/** The data seeded by the harness */
export function getSeed(): Seed {
    seed ??= readRunJson<Seed>('seed.json');
    return seed;
}

export function photo(path: string): SeedPhoto {
    const found = getSeed().photos[path];
    if (!found) throw new Error(`No seeded photo at ${path}`);
    return found;
}

export function folderId(name: string): number {
    const id = getSeed().folders[name];
    if (id === undefined) throw new Error(`No seeded folder named ${name}`);
    return id;
}
