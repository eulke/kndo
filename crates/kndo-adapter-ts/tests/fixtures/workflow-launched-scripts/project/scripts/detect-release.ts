import { releaseTag } from "./releaseUtils";

const tag = releaseTag(process.argv[2] ?? "");
console.log(`release=${tag}`);
