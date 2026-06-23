import fs from "node:fs";
import path from "node:path";

import type { CompanionArtifact } from "../types.js";

export function writeArtifact(baseDir: string, artifact: CompanionArtifact): string {
  const date = new Date(artifact.created_at);
  const dir = path.join(
    baseDir,
    String(date.getUTCFullYear()),
    String(date.getUTCMonth() + 1).padStart(2, "0"),
  );

  fs.mkdirSync(dir, { recursive: true });
  const filePath = path.join(dir, `${artifact.id}.json`);
  fs.writeFileSync(filePath, JSON.stringify(artifact, null, 2), "utf8");
  return filePath;
}

export function readArtifact(filePath: string): CompanionArtifact {
  return JSON.parse(fs.readFileSync(filePath, "utf8")) as CompanionArtifact;
}
