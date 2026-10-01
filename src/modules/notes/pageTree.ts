import type { SpacePageNode } from "./api";

export function findPage(nodes: SpacePageNode[], id: string): SpacePageNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    const found = findPage(node.children, id);
    if (found) return found;
  }
  return null;
}

export function pagePath(nodes: SpacePageNode[], id: string): SpacePageNode[] {
  for (const node of nodes) {
    if (node.id === id) return [node];
    const path = pagePath(node.children, id);
    if (path.length) return [node, ...path];
  }
  return [];
}

export function flattenPages(nodes: SpacePageNode[], prefix = ""): { node: SpacePageNode; path: string }[] {
  return nodes.flatMap((node) => {
    const path = prefix ? `${prefix} / ${node.title}` : node.title;
    return [{ node, path }, ...flattenPages(node.children, path)];
  });
}
