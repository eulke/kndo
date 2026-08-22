// kndo-action's publishing half (RFC 0010 §4): reads the JSON report — the only contract this
// frontend consumes — and publishes it to the three surfaces: the sticky PR comment (upserted
// in place, marked by a hidden HTML comment), file annotations for new findings ≥ warning
// (capped at GitHub's per-step limit), and the job summary (always, even where comments are
// impossible). Zero dependencies; node 20+.
//
// Env: KNDO_REPORT (path), KNDO_EXIT, KNDO_FAIL_ON, KNDO_COMMENT ("true"/"false"),
// GITHUB_TOKEN, plus the standard GitHub runner environment.
//
// kndo:allow-file unused invoked by action.yml via node — an entry point no import graph reaches
// kndo:allow-file crap presentation-only frontend script, exercised end to end by every action run

import { readFileSync, appendFileSync } from "node:fs";

const MARKER = "<!-- kndo-report -->";
const ANNOTATION_CAP = 10; // GitHub renders ~10 annotations per step; honored explicitly (§4).
const SECTION_CAP = 30; // hard cap per comment section, with a link to the run for the rest.

const report = JSON.parse(readFileSync(process.env.KNDO_REPORT, "utf8"));
const exitCode = Number(process.env.KNDO_EXIT ?? "0");

// RFC 0009 §3's glyph vocabulary, emoji-safe (render.rs `glyph`); groups drive section order.
const GROUP_ORDER = ["defect", "waste", "risk", "hygiene", "convention"];
const GLYPH = { defect: "✗", waste: "◦", risk: "▲", hygiene: "·", convention: "•" };

const findings = report.findings ?? [];
const fixed = report.fixed ?? [];
const diffMode = (report.run?.mode ?? "full") !== "full";

function groupRank(g) {
  const i = GROUP_ORDER.indexOf(g);
  return i === -1 ? GROUP_ORDER.length : i;
}

function where(f) {
  const path = f.location?.path;
  if (!path) return f.location?.symbol ?? f.location?.package ?? "—";
  const line = f.location?.range?.start?.[0];
  return line ? `${path}:${line}` : path;
}

function mdEscape(s) {
  return String(s).replace(/\|/g, "\\|").replace(/\n/g, " ");
}

function findingRow(f) {
  const glyph = GLYPH[f.group] ?? "•";
  const advisory = f.advisory ? " (advisory)" : "";
  const label = `\`${f.category}\` ${f.subject_kind}${advisory}`;
  return `| ${glyph} | ${label} | \`${mdEscape(where(f))}\` | ${mdEscape(f.message)} |`;
}

function healthLine() {
  const h = report.health;
  if (!h) return "";
  const prev = h.previous?.score;
  if (prev == null) return `health ${h.score} (${h.grade})`;
  const arrow = h.score > prev ? "↑" : h.score < prev ? "↓" : "→";
  return `health ${prev} → ${h.score} (${h.grade}) ${arrow}`;
}

function buildMarkdown() {
  const runUrl = `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`;
  const parts = [];
  const headBits = [];
  if (diffMode) {
    headBits.push(`${findings.length} new`, `${fixed.length} fixed`);
  } else {
    headBits.push(`${findings.length} finding${findings.length === 1 ? "" : "s"}`);
  }
  const health = healthLine();
  if (health) headBits.push(health);
  parts.push(`### kndo · ${headBits.join(" · ")}`);

  if (findings.length > 0) {
    const sorted = [...findings].sort(
      (a, b) => groupRank(a.group) - groupRank(b.group) || a.id.localeCompare(b.id),
    );
    const shown = sorted.slice(0, SECTION_CAP);
    parts.push(
      diffMode ? "**New**" : "**Findings**",
      "| | finding | where | why |",
      "|-|---------|-------|-----|",
      ...shown.map(findingRow),
    );
    if (sorted.length > shown.length) {
      parts.push(`…and ${sorted.length - shown.length} more — [full report in the workflow run](${runUrl})`);
    }
  } else if (diffMode) {
    parts.push("No new findings.");
  }

  if (fixed.length > 0) {
    const shown = fixed.slice(0, SECTION_CAP);
    // Fixed findings always render — the reward loop applies to reviewers too (§4).
    parts.push(
      "**Fixed** " +
        shown
          .map((f) => `✓ \`${f.category}\` ${f.subject_kind} \`${mdEscape(f.location?.symbol ?? where(f))}\``)
          .join(" · "),
    );
    if (fixed.length > shown.length) parts.push(`…and ${fixed.length - shown.length} more fixed`);
  }

  const acknowledged = report.baseline?.acknowledged;
  if (acknowledged) {
    parts.push(`<details><summary>${acknowledged} baseline finding${acknowledged === 1 ? "" : "s"} unchanged</summary>\n\nAcknowledged at adoption time (\`.kndo/baseline.json\`); not shown here.\n</details>`);
  }

  const diagnostics = report.diagnostics ?? [];
  if (diagnostics.length > 0) {
    parts.push(
      `<details><summary>${diagnostics.length} diagnostic${diagnostics.length === 1 ? "" : "s"}</summary>\n`,
      ...diagnostics.slice(0, SECTION_CAP).map((d) => `- ${mdEscape(d.message)}`),
      "</details>",
    );
  }

  const supp = report.suppressed;
  if (supp && supp.inline + supp.config > 0) {
    parts.push(`_${supp.inline + supp.config} finding(s) suppressed (inline: ${supp.inline}, config: ${supp.config})_`);
  }

  parts.push("", `<sub>[run](${runUrl}) · mode \`${report.run?.mode}\` · fail-on \`${process.env.KNDO_FAIL_ON}\` · kndo ${report.kndo_version}</sub>`);
  parts.push(MARKER);
  return parts.join("\n");
}

function emitAnnotations() {
  const severe = findings.filter(
    (f) => (f.severity === "warning" || f.severity === "error") && !f.advisory && f.location?.path,
  );
  for (const f of severe.slice(0, ANNOTATION_CAP)) {
    const line = f.location.range?.start?.[0] ?? 1;
    const endLine = f.location.range?.end?.[0] ?? line;
    const level = f.severity === "error" ? "error" : "warning";
    // Workflow-command message: escape %, \r, \n per the commands spec.
    const msg = String(f.message).replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
    console.log(
      `::${level} file=${f.location.path},line=${line},endLine=${endLine},title=kndo ${f.category}::${msg}`,
    );
  }
  if (severe.length > ANNOTATION_CAP) {
    console.log(
      `::notice title=kndo::${severe.length - ANNOTATION_CAP} more finding(s) not annotated (GitHub per-step cap) — see the PR comment / job summary`,
    );
  }
}

async function upsertComment(body) {
  if (process.env.KNDO_COMMENT !== "true") return;
  let event = {};
  try {
    event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
  } catch {
    /* no event payload — not a workflow context that can comment */
  }
  const pr = event.pull_request?.number;
  if (!pr) return; // not a PR run — annotations + summary already cover it (§4).

  const api = process.env.GITHUB_API_URL ?? "https://api.github.com";
  const repo = process.env.GITHUB_REPOSITORY;
  const headers = {
    Authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "User-Agent": "kndo-action",
  };

  try {
    let existing = null;
    for (let page = 1; page <= 5 && !existing; page++) {
      const res = await fetch(`${api}/repos/${repo}/issues/${pr}/comments?per_page=100&page=${page}`, { headers });
      if (!res.ok) throw new Error(`list comments: HTTP ${res.status}`);
      const comments = await res.json();
      existing = comments.find((c) => typeof c.body === "string" && c.body.includes(MARKER)) ?? null;
      if (comments.length < 100) break;
    }
    const res = existing
      ? await fetch(`${api}/repos/${repo}/issues/comments/${existing.id}`, {
          method: "PATCH",
          headers,
          body: JSON.stringify({ body }),
        })
      : await fetch(`${api}/repos/${repo}/issues/${pr}/comments`, {
          method: "POST",
          headers,
          body: JSON.stringify({ body }),
        });
    if (!res.ok) throw new Error(`write comment: HTTP ${res.status}`);
  } catch (e) {
    // Fork PRs: the default token cannot write comments — degrade, never fail (§5).
    const notice = `kndo: could not publish the PR comment (${e.message}) — likely a fork PR without a write token; the report is in this job summary instead.`;
    console.log(`::notice title=kndo::${notice}`);
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n> ${notice}\n`);
  }
}

const markdown = buildMarkdown();
appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown + "\n");
if (exitCode !== 2) emitAnnotations();
await upsertComment(markdown);
