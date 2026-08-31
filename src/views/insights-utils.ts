export type InsightsTab = "usage" | "voice";

export type ActivityPoint = {
  day: string;
  sessions: number;
};

export function formatInsightNumber(value: number, maximumFractionDigits = 0) {
  return new Intl.NumberFormat("pt-BR", { maximumFractionDigits }).format(value);
}

export function buildActivityCells(activity: ActivityPoint[], now = new Date()) {
  const counts = new Map(activity.map((day) => [day.day, day.sessions]));
  const cells: Array<{ key: string; count: number }> = [];
  for (let offset = 90; offset >= 0; offset -= 1) {
    const date = new Date(now);
    date.setDate(now.getDate() - offset);
    const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
    cells.push({ key, count: counts.get(key) ?? 0 });
  }
  return cells;
}

export function adjacentInsightsTab(current: InsightsTab, key: string): InsightsTab {
  if (key === "Home" || key === "ArrowLeft") return "usage";
  if (key === "End" || key === "ArrowRight") return "voice";
  return current;
}

export function voiceProfileProgress(progressWords: number, requiredWords: number) {
  if (requiredWords <= 0) return 100;
  return Math.min(100, Math.max(0, progressWords / requiredWords * 100));
}

export function voiceProfileRemainingWords(progressWords: number, requiredWords: number) {
  return Math.max(0, requiredWords - progressWords);
}

export function voiceEvidenceLevelLabel(level: string) {
  const labels: Record<string, string> = {
    collecting: "Coletando evidências",
    basic: "Estatísticas básicas",
    archetype: "Archetype inicial",
    rich: "Perfil enriquecido",
    high_confidence: "Alta confiança",
  };
  return labels[level] ?? "Em aprendizado";
}
