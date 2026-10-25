/**
 * Checks Rust line coverage per crate against the floors in docs/engineering/testing.md.
 *
 * Input: the JSON report from `cargo llvm-cov --json --summary-only`.
 * Usage: node scripts/check-coverage.ts target/coverage.json
 */
import { readFileSync } from "node:fs";
import path from "node:path";

/** Minimum line coverage per crate folder, in percent. */
const FLOORS: Readonly<Record<string, number>> = {
  ir: 90,
  engine: 90,
  compiler: 85,
};

interface LineSummary {
  count: number;
  covered: number;
}

interface LlvmCovReport {
  data: { files: { filename: string; summary: { lines: LineSummary } }[] }[];
}

function crateOf(filename: string): string | undefined {
  const parts = path.relative(process.cwd(), filename).split(path.sep);
  return parts[0] === "crates" ? parts[1] : undefined;
}

function main(reportPath: string): boolean {
  const report = JSON.parse(readFileSync(reportPath, "utf8")) as LlvmCovReport;
  const totals = new Map<string, LineSummary>();
  for (const file of report.data.flatMap((d) => d.files)) {
    const crate = crateOf(file.filename);
    if (crate === undefined) continue;
    const total = totals.get(crate) ?? { count: 0, covered: 0 };
    total.count += file.summary.lines.count;
    total.covered += file.summary.lines.covered;
    totals.set(crate, total);
  }

  let ok = true;
  for (const [crate, floor] of Object.entries(FLOORS)) {
    const total = totals.get(crate) ?? { count: 0, covered: 0 };
    if (total.count === 0) {
      console.log(`${crate}: no executable code yet (floor ${String(floor)}%)`);
      continue;
    }
    const percent = (100 * total.covered) / total.count;
    const passed = percent >= floor;
    ok &&= passed;
    console.log(
      `${crate}: ${percent.toFixed(1)}% of ${String(total.count)} lines (floor ${String(floor)}%) ${passed ? "ok" : "BELOW FLOOR"}`,
    );
  }
  return ok;
}

const reportPath = process.argv[2];
if (reportPath === undefined) {
  console.error("usage: node scripts/check-coverage.ts <llvm-cov JSON report>");
  process.exit(2);
}
if (!main(reportPath)) process.exit(1);
