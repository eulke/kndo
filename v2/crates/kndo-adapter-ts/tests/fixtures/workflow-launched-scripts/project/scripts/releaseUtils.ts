export function releaseTag(subject: string): string {
  return subject.startsWith("release:") ? subject.slice("release:".length).trim() : "";
}
