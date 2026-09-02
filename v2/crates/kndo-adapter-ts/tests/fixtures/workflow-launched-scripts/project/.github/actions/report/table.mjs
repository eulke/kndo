export function table(rows) {
  return rows.map((cells) => `| ${cells.join(" | ")} |`).join("\n");
}
