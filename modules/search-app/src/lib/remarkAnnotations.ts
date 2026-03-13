import { visit } from "unist-util-visit";
import type { Plugin } from "unified";
import type { Root, Text, PhrasingContent } from "mdast";

// Matches [signal:UUID] tokens — UUID is a 36-char hex-dash pattern
const ANNOTATION_RE = /\[([a-zA-Z]+):([^\]\[]+)\]/g;

/**
 * Remark plugin that transforms [type:identifier] annotations in markdown text
 * into custom `citation` MDAST nodes that ReactMarkdown renders via component overrides.
 */
const remarkAnnotations: Plugin<[], Root> = () => {
  return (tree: Root) => {
    visit(tree, "text", (node: Text, index, parent) => {
      if (!parent || index === undefined) return;

      const value = node.value;
      ANNOTATION_RE.lastIndex = 0;

      const matches: { start: number; end: number; kind: string; id: string }[] = [];
      let match;
      while ((match = ANNOTATION_RE.exec(value)) !== null) {
        matches.push({
          start: match.index,
          end: match.index + match[0].length,
          kind: match[1]!,
          id: match[2]!,
        });
      }

      if (matches.length === 0) return;

      // Split the text node into alternating text + citation nodes
      const children: PhrasingContent[] = [];
      let lastEnd = 0;

      for (const m of matches) {
        if (m.start > lastEnd) {
          children.push({ type: "text", value: value.slice(lastEnd, m.start) });
        }
        // Custom MDAST node — ReactMarkdown renders via components.citation
        children.push({
          type: "citation" as "text",
          data: {
            hName: "citation",
            hProperties: { kind: m.kind, identifier: m.id },
          },
        } as unknown as PhrasingContent);
        lastEnd = m.end;
      }

      if (lastEnd < value.length) {
        children.push({ type: "text", value: value.slice(lastEnd) });
      }

      // Replace the original text node with the split children
      parent.children.splice(index, 1, ...children);
    });
  };
};

export default remarkAnnotations;
