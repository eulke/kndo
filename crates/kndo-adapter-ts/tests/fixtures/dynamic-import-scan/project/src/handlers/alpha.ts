// Alive only through the narrowed dynamic import — no static import names this file, and no
// binding names `run`; the target-side wildcard is what keeps both out of `unused`.
export function run(): string {
  return "alpha";
}
