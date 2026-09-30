const K256 = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
]);

function rotr(n: number, s: number): number {
    return (n >>> s) | (n << (32 - s));
}

export function sha256Hex(input: string): string {
    const bytes = new TextEncoder().encode(input);
    const byteLen = bytes.length;
    const bitLen = byteLen * 8;

    const fullLen = Math.ceil((byteLen + 9) / 64) * 64;
    const padded = new Uint8Array(fullLen);
    padded.set(bytes);
    padded[byteLen] = 0x80;
    const view = new DataView(padded.buffer);
    view.setUint32(fullLen - 8, Math.floor(bitLen / 0x100000000), false);
    view.setUint32(fullLen - 4, bitLen, false);

    let h0 = 0x6a09e667, h1 = 0xbb67ae85, h2 = 0x3c6ef372, h3 = 0xa54ff53a;
    let h4 = 0x510e527f, h5 = 0x9b05688c, h6 = 0x1f83d9ab, h7 = 0x5be0cd19;

    const w = new Uint32Array(64);

    for (let chunk = 0; chunk < fullLen; chunk += 64) {
        for (let t = 0; t < 16; t++) {
            w[t] = view.getUint32(chunk + t * 4, false);
        }
        for (let t = 16; t < 64; t++) {
            const s0 = rotr(w[t - 15], 7) ^ rotr(w[t - 15], 18) ^ (w[t - 15] >>> 3);
            const s1 = rotr(w[t - 2], 17) ^ rotr(w[t - 2], 19) ^ (w[t - 2] >>> 10);
            w[t] = (w[t - 16] + s0 + w[t - 7] + s1) | 0;
        }

        let a = h0, b = h1, c = h2, d = h3, e = h4, f = h5, g = h6, h = h7;

        for (let t = 0; t < 64; t++) {
            const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
            const ch = (e & f) ^ ((~e) & g);
            const temp1 = (h + S1 + ch + K256[t] + w[t]) | 0;
            const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
            const maj = (a & b) ^ (a & c) ^ (b & c);
            const temp2 = (S0 + maj) | 0;

            h = g;
            g = f;
            f = e;
            e = (d + temp1) | 0;
            d = c;
            c = b;
            b = a;
            a = (temp1 + temp2) | 0;
        }

        h0 = (h0 + a) | 0;
        h1 = (h1 + b) | 0;
        h2 = (h2 + c) | 0;
        h3 = (h3 + d) | 0;
        h4 = (h4 + e) | 0;
        h5 = (h5 + f) | 0;
        h6 = (h6 + g) | 0;
        h7 = (h7 + h) | 0;
    }

    const out = new DataView(new ArrayBuffer(32));
    out.setUint32(0, h0, false);
    out.setUint32(4, h1, false);
    out.setUint32(8, h2, false);
    out.setUint32(12, h3, false);
    out.setUint32(16, h4, false);
    out.setUint32(20, h5, false);
    out.setUint32(24, h6, false);
    out.setUint32(28, h7, false);

    return Array.from(new Uint8Array(out.buffer))
        .map(b => b.toString(16).padStart(2, '0'))
        .join('');
}

function getKeyHashSuffix(key: string): string {
    const trimmed = key.trim();
    if (!trimmed) return '';
    return sha256Hex(trimmed).slice(-6);
}

function normalizeBaseUrl(url: string): string {
    let u = (url || '').trim().replace(/\/+$/, '');
    if (!u) return '';
    if (!u.endsWith('/v1')) {
        u = `${u}/v1`;
    }
    return u;
}

export interface OpencodeProviderSummary {
    id: string;
    name?: string;
    npm?: string;
    baseUrl?: string;
    apiKey?: string;
    models: string[];
}

export type ProfileSyncStatus = 'not_present' | 'partial' | 'synced';

export function getProfileInfo(opencodeProviders: OpencodeProviderSummary[], keyToCheck: string, urlToCheck: string, modelsToCheck?: string[]) {
    const trimmedKey = keyToCheck.trim();
    if (!trimmedKey) {
        return {
            suffix: '',
            providerId: '',
            providerName: '',
            status: 'not_present' as ProfileSyncStatus,
            existing: undefined
        };
    }
    const suffix = getKeyHashSuffix(trimmedKey);
    const shortId = `apikey-fun-${suffix}`;
    const fullId = `apikey-fun-${sha256Hex(trimmedKey)}`;
    // A 24-bit suffix can collide. Never overwrite a different key's profile.
    const shortProfile = opencodeProviders.find(p => p.id === shortId);
    const providerId = shortProfile && shortProfile.apiKey?.trim() !== trimmedKey ? fullId : shortId;
    const providerName = `APIKEY.FUN (${suffix})`;

    // Match either by suffixed id or legacy 'apikey-fun' if apiKey matches
    const existing = opencodeProviders.find(p => p.id === fullId && p.apiKey?.trim() === trimmedKey)
        ?? opencodeProviders.find(p => p.id === providerId && p.apiKey?.trim() === trimmedKey)
        ?? opencodeProviders.find(p => p.id === 'apikey-fun' && p.apiKey?.trim() === trimmedKey);

    if (!existing) {
        return {
            suffix,
            providerId,
            providerName,
            status: 'not_present' as ProfileSyncStatus,
            existing: undefined
        };
    }

    const urlMatches = normalizeBaseUrl(existing.baseUrl || '') === normalizeBaseUrl(urlToCheck);
    const keyMatches = (existing.apiKey || '').trim() === trimmedKey;

    const existingModels = (existing.models ?? []).map(m => m.trim()).filter(Boolean);

    let modelsMatch = true;
    if (modelsToCheck !== undefined) {
        const cleanedCheck = modelsToCheck.map(m => m.trim()).filter(Boolean);
        const checkSet = new Set(cleanedCheck);
        const existingSet = new Set(existingModels);
        modelsMatch = checkSet.size === existingSet.size && cleanedCheck.every(m => existingSet.has(m));
    } else if (existingModels.length === 0) {
        // Profile has no models in OpenCode, consider it partial/needs sync
        modelsMatch = false;
    }

    const status: ProfileSyncStatus = (urlMatches && keyMatches && modelsMatch && existing.npm === '@ai-sdk/openai-compatible') ? 'synced' : 'partial';

    return {
        suffix,
        providerId: existing.id,
        providerName: existing.name || providerName,
        status,
        existing
    };
}
