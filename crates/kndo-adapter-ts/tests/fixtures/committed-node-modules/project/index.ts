import { leftPad } from "left-pad";

export function pad(value: string): string {
  return leftPad(value, 8);
}

function helper(): string {
  return "never called";
}
