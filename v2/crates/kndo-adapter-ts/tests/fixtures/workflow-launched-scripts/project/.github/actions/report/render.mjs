import { table } from "./table.mjs";

console.log(table([["notes", process.env.NOTES_FILE ?? ""]]));
