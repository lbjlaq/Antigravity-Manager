export function parseTags(label?: string | null): string[] {
    if (!label || !label.trim()) return [];
    return label
        .split(/[,，]/)
        .map(t => t.trim())
        .filter(t => t.length > 0);
}

export function formatTags(tags: string[]): string {
    return tags
        .map(t => t.trim())
        .filter(t => t.length > 0)
        .join(', ');
}

export function reorderTags(tags: string[], fromIndex: number, toIndex: number): string[] {
    if (
        fromIndex < 0 ||
        fromIndex >= tags.length ||
        toIndex < 0 ||
        toIndex >= tags.length ||
        fromIndex === toIndex
    ) {
        return [...tags];
    }
    const result = [...tags];
    const [moved] = result.splice(fromIndex, 1);
    result.splice(toIndex, 0, moved);
    return result;
}
