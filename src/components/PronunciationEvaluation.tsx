import { useMemo, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import {
  Sparkles,
  TrendingUp,
  AlertTriangle,
  Mic2,
  Gauge,
  SpellCheck,
  BookOpen,
  MessageSquare,
  Quote,
  Target,
  Gavel,
  ShieldQuestion,
} from "lucide-react";

/**
 * Renders the Gemini pronunciation assessment returned by the backend.
 *
 * The prompt (see `gemini.rs`) forces a fixed Markdown structure so this
 * component can parse it into discrete sections and give each one its own
 * visual treatment — a hero scorecard for the Placar, icon-accented cards for
 * the qualitative sections, etc. When the Markdown cannot be parsed (older
 * feedback format or a model hiccup), we gracefully fall back to a plain
 * styled render of the raw text.
 */

/** Ordered list of every section the prompt asks for. */
const SECTION_TITLES = [
  "Resumo Executivo",
  "Placar",
  "Forças",
  "Pontos de Atenção",
  "Pronúncia e Inteligibilidade",
  "Fluência e Ritmo",
  "Gramática Oral e Estrutura",
  "Vocabulário e Adequação",
  "Naturalidade e Registro",
  "Evidências do Áudio",
  "Plano de Melhoria",
  "Veredito Final",
] as const;

type SectionTitle = (typeof SECTION_TITLES)[number];

/** Metadata used to render each section's header and accent. */
const SECTION_META: Record<
  SectionTitle,
  { icon: typeof Sparkles; accent: string }
> = {
  "Resumo Executivo": { icon: Sparkles, accent: "text-coral-300" },
  Placar: { icon: Gauge, accent: "text-coral-300" },
  Forças: { icon: TrendingUp, accent: "text-emerald-400" },
  "Pontos de Atenção": { icon: AlertTriangle, accent: "text-amber-400" },
  "Pronúncia e Inteligibilidade": { icon: Mic2, accent: "text-sky-400" },
  "Fluência e Ritmo": { icon: Gauge, accent: "text-violet-400" },
  "Gramática Oral e Estrutura": { icon: SpellCheck, accent: "text-rose-400" },
  "Vocabulário e Adequação": { icon: BookOpen, accent: "text-teal-400" },
  "Naturalidade e Registro": { icon: MessageSquare, accent: "text-fuchsia-400" },
  "Evidências do Áudio": { icon: Quote, accent: "text-zinc-300" },
  "Plano de Melhoria": { icon: Target, accent: "text-coral-300" },
  "Veredito Final": { icon: Gavel, accent: "text-coral-300" },
};

interface ParsedEvaluation {
  sections: Map<SectionTitle, string>;
  /** Raw, untouched Markdown, used for the graceful fallback. */
  raw: string;
}

/**
 * Splits the Markdown into a map of { section title -> body }.
 *
 * The prompt guarantees `## Title` headings on their own line, so a simple
 * line-based scan is enough. Anything appearing before the first known
 * heading is treated as preamble and ignored (the prompt forbids it, but we
 * stay defensive).
 */
function parseEvaluation(markdown: string): ParsedEvaluation {
  const sections = new Map<SectionTitle, string>();
  const known = new Set<string>(SECTION_TITLES);

  const lines = markdown.replace("\r\n", "\n").split("\n");
  let current: SectionTitle | null = null;
  let buffer: string[] = [];

  const flush = () => {
    if (current) {
      sections.set(current, buffer.join("\n").trim());
    }
    buffer = [];
  };

  for (const line of lines) {
    const match = /^##\s+(.+?)\s*$/.exec(line);
    if (match && known.has(match[1])) {
      flush();
      current = match[1] as SectionTitle;
    } else if (current) {
      buffer.push(line);
    }
  }
  flush();

  return { sections, raw: markdown };
}

/* ------------------------------- Placar ------------------------------- */

interface Scoreboard {
  grade: number | null; // 0..10
  cefr: string | null; // A1..C2
  reference: string | null;
  nativeProximity: number | null; // 0..100
  confidence: "baixa" | "média" | "alta" | null;
}

/** CEFR level -> display colour and ordinal used for the step indicator. */
const CEFR_LEVELS = ["A1", "A2", "B1", "B2", "C1", "C2"] as const;

function cefrStyle(level: string | null): { color: string; index: number } {
  if (!level) return { color: "text-zinc-400", index: -1 };
  const idx = CEFR_LEVELS.indexOf(
    level.toUpperCase().trim() as (typeof CEFR_LEVELS)[number],
  );
  switch (idx) {
    case 0:
    case 1:
      return { color: "text-rose-400", index: idx };
    case 2:
    case 3:
      return { color: "text-amber-400", index: idx };
    case 4:
      return { color: "text-emerald-400", index: idx };
    case 5:
      return { color: "text-sky-400", index: idx };
    default:
      return { color: "text-zinc-300", index: idx };
  }
}

/** Parses the "Placar" bullet list into typed values for the hero scorecard. */
function parseScoreboard(body: string): Scoreboard {
  const scoreboard: Scoreboard = {
    grade: null,
    cefr: null,
    reference: null,
    nativeProximity: null,
    confidence: null,
  };

  const lines = body.split("\n");
  for (const line of lines) {
    const text = line.replace(/^[-*]\s*/, "").trim();
    if (!text) continue;

    // Nota geral: 7,5/10  | 7.5/10  | 7,5  | 7.5
    const gradeMatch = /nota\s+geral\s*:\s*(\d+[.,]?\d*)\s*(?:\/\s*10)?/i.exec(
      text,
    );
    if (gradeMatch) {
      scoreboard.grade = parseFloat(gradeMatch[1].replace(",", "."));
      continue;
    }

    const cefrMatch = /cefr\s+estimad[oa]\s*:\s*([a-c][12])/i.exec(text);
    if (cefrMatch) {
      scoreboard.cefr = cefrMatch[1].toUpperCase();
      continue;
    }

    const refMatch = /refer[êe]ncia\s+internacional\s+de\s+fala\s*:\s*(.+)/i.exec(
      text,
    );
    if (refMatch) {
      scoreboard.reference = refMatch[1].trim();
      continue;
    }

    const proxMatch = /proximidade\s+de\s+fala\s+nativ[ao]\s*:\s*(\d{1,3})/i.exec(
      text,
    );
    if (proxMatch) {
      const v = parseInt(proxMatch[1], 10);
      if (!Number.isNaN(v)) scoreboard.nativeProximity = v;
      continue;
    }

    const confMatch = /confian[çc]a\s+da\s+avalia[çc][ãa]o\s*:\s*(baixa|m[ée]dia|alta)/i.exec(
      text,
    );
    if (confMatch) {
      const raw = confMatch[1].toLowerCase();
      scoreboard.confidence =
        raw === "baixa" ? "baixa" : raw === "alta" ? "alta" : "média";
      continue;
    }
  }

  return scoreboard;
}

function StatTile({
  label,
  value,
  children,
}: {
  label: string;
  value?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-zinc-800/70 bg-zinc-950/50 px-3 py-4 text-center">
      <span className="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">
        {label}
      </span>
      {value !== undefined && (
        <span className="mt-1.5 text-lg font-bold text-zinc-100">{value}</span>
      )}
      {children}
    </div>
  );
}

function ScoreboardHero({ body }: { body: string }) {
  const s = useMemo(() => parseScoreboard(body), [body]);
  const cefr = cefrStyle(s.cefr);

  const gradeClamped =
    s.grade === null ? null : Math.max(0, Math.min(10, s.grade));
  const proxClamped =
    s.nativeProximity === null ? null : Math.max(0, Math.min(100, s.nativeProximity));

  return (
    <div className="overflow-hidden rounded-2xl border border-coral-500/30 bg-gradient-to-br from-coral-500/10 via-zinc-900/60 to-zinc-950">
      <div className="grid gap-4 p-6 sm:grid-cols-[auto_1fr]">
        {/* Big grade dial */}
        <div className="flex flex-col items-center justify-center">
          <div className="relative flex h-32 w-32 items-center justify-center rounded-full border-4 border-zinc-800/80 bg-zinc-950">
            <div
              className="absolute inset-0 rounded-full"
              style={{
                background: `conic-gradient(#E14D2A ${
                  gradeClamped === null ? 0 : (gradeClamped / 10) * 360
                }deg, rgba(39,39,42,0.6) 0deg)`,
                mask: "radial-gradient(farthest-side, transparent calc(100% - 8px), #000 calc(100% - 8px))",
                WebkitMask:
                  "radial-gradient(farthest-side, transparent calc(100% - 8px), #000 calc(100% - 8px))",
              }}
            />
            <div className="relative flex flex-col items-center">
              <span className="font-mono text-3xl font-bold text-coral-300">
                {gradeClamped === null ? "—" : gradeClamped.toFixed(1)}
              </span>
              <span className="text-[10px] font-medium uppercase tracking-wider text-zinc-500">
                / 10
              </span>
            </div>
          </div>
          <span className="mt-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
            Nota geral
          </span>
        </div>

        {/* Stat tiles */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-2">
          <StatTile label="CEFR estimado">
            <span className={`text-xl font-bold ${cefr.color}`}>{s.cefr ?? "—"}</span>
            {cefr.index >= 0 && (
              <div className="mt-2 flex justify-center gap-1">
                {CEFR_LEVELS.map((lvl, i) => (
                  <span
                    key={lvl}
                    className={
                      "h-1.5 w-4 rounded-full " +
                      (i <= cefr.index ? "bg-coral-500" : "bg-zinc-800")
                    }
                  />
                ))}
              </div>
            )}
          </StatTile>

          <StatTile
            label="Proximidade nativa"
            value={
              proxClamped === null ? (
                "—"
              ) : (
                <span className="text-coral-300">{proxClamped}/100</span>
              )
            }
          >
            {proxClamped !== null && (
              <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-coral-500 to-amber-400"
                  style={{ width: `${proxClamped}%` }}
                />
              </div>
            )}
          </StatTile>

          <StatTile
            label="Referência"
            value={
              <span className="text-sm font-semibold leading-tight text-zinc-200">
                {s.reference ?? "—"}
              </span>
            }
          />

          <StatTile
            label="Confiança"
            value={
              <span
                className={
                  "text-sm font-semibold " +
                  (s.confidence === "alta"
                    ? "text-emerald-400"
                    : s.confidence === "média"
                      ? "text-amber-400"
                      : s.confidence === "baixa"
                        ? "text-rose-400"
                        : "text-zinc-300")
                }
              >
                {s.confidence ? s.confidence[0].toUpperCase() + s.confidence.slice(1) : "—"}
              </span>
            }
          />
        </div>
      </div>
    </div>
  );
}

/* ----------------------------- List sections ----------------------------- */

/** Renders Forças / Pontos de Atenção as a styled checkered list. */
function CheckListSection({
  body,
  variant,
}: {
  body: string;
  variant: "positive" | "warning";
}) {
  const items = useMemo(
    () =>
      body
        .split("\n")
        .map((l) => l.replace(/^[-*]\s*/, "").trim())
        .filter(Boolean),
    [body],
  );

  const dot =
    variant === "positive"
      ? "bg-emerald-500/15 text-emerald-400"
      : "bg-amber-500/15 text-amber-400";

  return (
    <ul className="space-y-2">
      {items.map((item, i) => (
        <li
          key={i}
          className="flex items-start gap-3 rounded-xl border border-zinc-800/50 bg-zinc-950/40 px-4 py-3"
        >
          <span
            className={
              "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md " +
              dot
            }
          >
            {variant === "positive" ? "✓" : "!"}
          </span>
          <span className="text-sm leading-relaxed text-zinc-300">
            <InlineMarkdown>{item}</InlineMarkdown>
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Renders the Plano de Melhoria, splitting it into the two requested groups. */
function ImprovementSection({ body }: { body: string }) {
  const { actions, exercises } = useMemo(() => {
    const lines = body.split("\n").map((l) => l.trim()).filter(Boolean);
    const actions: string[] = [];
    const exercises: string[] = [];
    let target: "actions" | "exercises" | null = null;

    for (const line of lines) {
      // Display text: strip only the leading list/number marker so inline
      // Markdown (bold, italics) is preserved when rendered.
      const cleaned = line
        .replace(/^[-*+]\s*/, "")
        .replace(/^\d+[.)]\s*/, "")
        .trim();
      if (!cleaned) continue;

      // Classification text: also peel headings and emphasis markers so a
      // sub-heading written as "- **Exercícios específicos...**" is
      // recognised as a group separator instead of leaking into the list as
      // an item (the previous code rejected any heading that carried a list
      // marker, which is exactly how the model formats it).
      const normalized = cleaned
        .replace(/^#{1,6}\s*/, "")
        .replace(/^\*{1,2}/, "")
        .replace(/\*{1,2}$/, "")
        .replace(/[_]/g, "")
        .replace(/[:：]\s*$/, "")
        .trim();

      if (/^exerc[ií]cios?\b/i.test(normalized)) {
        target = "exercises";
        continue;
      }
      if (/^a[çc][õo]es?\s+pr[áa]ticas/i.test(normalized)) {
        target = "actions";
        continue;
      }

      // Heuristic: the first 5 items are actions, the next 3 are exercises,
      // unless a sub-heading already routed them.
      if (target === "exercises") exercises.push(cleaned);
      else if (target === "actions") actions.push(cleaned);
      else if (actions.length < 5) actions.push(cleaned);
      else exercises.push(cleaned);
    }
    return { actions, exercises };
  }, [body]);

  return (
    <div className="space-y-5">
      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
          5 ações práticas e priorizadas
        </h4>
        <ol className="space-y-2">
          {actions.map((a, i) => (
            <li
              key={i}
              className="flex items-start gap-3 rounded-xl border border-zinc-800/50 bg-zinc-950/40 px-4 py-3"
            >
              <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-coral-500/15 font-mono text-[11px] font-bold text-coral-300">
                {i + 1}
              </span>
              <span className="text-sm leading-relaxed text-zinc-300">
                <InlineMarkdown>{a}</InlineMarkdown>
              </span>
            </li>
          ))}
        </ol>
      </div>

      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
          3 exercícios para subir um nível
        </h4>
        <ol className="space-y-2">
          {exercises.map((ex, i) => (
            <li
              key={i}
              className="flex items-start gap-3 rounded-xl border border-zinc-800/50 bg-zinc-950/40 px-4 py-3"
            >
              <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-emerald-500/15 text-emerald-400">
                <Target className="h-3 w-3" />
              </span>
              <span className="text-sm leading-relaxed text-zinc-300">
                <InlineMarkdown>{ex}</InlineMarkdown>
              </span>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}

/** Lightweight inline-only Markdown (bold, italics, code) for list items. */
function InlineMarkdown({ children }: { children: string }) {
  return (
    <ReactMarkdown
      components={{
        p: ({ children }) => <span>{children}</span>,
        strong: ({ children }) => (
          <strong className="font-semibold text-zinc-100">{children}</strong>
        ),
        em: ({ children }) => <em className="italic text-zinc-200">{children}</em>,
        code: ({ children }) => (
          <code className="rounded bg-zinc-800/80 px-1 py-0.5 font-mono text-[12px] text-coral-300">
            {children}
          </code>
        ),
      }}
    >
      {children}
    </ReactMarkdown>
  );
}

/** Generic prose block renderer for the descriptive sections. */
function ProseSection({ body }: { body: string }) {
  return (
    <div className="rounded-xl border border-zinc-800/50 bg-zinc-950/40 px-5 py-4">
      <div className="space-y-2 text-sm leading-relaxed text-zinc-300">
        <ReactMarkdown
          components={{
            p: ({ children }) => <p>{children}</p>,
            strong: ({ children }) => (
              <strong className="font-semibold text-zinc-100">{children}</strong>
            ),
            em: ({ children }) => <em className="italic text-zinc-200">{children}</em>,
            ul: ({ children }) => (
              <ul className="ml-5 list-disc space-y-1">{children}</ul>
            ),
            ol: ({ children }) => (
              <ol className="ml-5 list-decimal space-y-1">{children}</ol>
            ),
            li: ({ children }) => <li>{children}</li>,
            code: ({ children }) => (
              <code className="rounded bg-zinc-800/80 px-1 py-0.5 font-mono text-[12px] text-coral-300">
                {children}
              </code>
            ),
          }}
        >
          {body}
        </ReactMarkdown>
      </div>
    </div>
  );
}

/* ----------------------------- Section wrapper ----------------------------- */

function SectionCard({
  title,
  icon: Icon,
  accent,
  children,
}: {
  title: string;
  icon: typeof Sparkles;
  accent: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-3">
      <div className="flex items-center gap-2.5">
        <Icon className={`h-4 w-4 ${accent}`} />
        <h4 className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
          {title}
        </h4>
      </div>
      {children}
    </section>
  );
}

/* ------------------------------- Main entry ------------------------------- */

export function PronunciationEvaluation({ markdown }: { markdown: string }) {
  const parsed = useMemo(() => parseEvaluation(markdown), [markdown]);
  const sections = parsed.sections;

  // Graceful fallback: if none of the expected headings were found, render the
  // raw Markdown in a simple styled container so the user still sees something.
  if (sections.size === 0) {
    return (
      <div className="rounded-xl border border-zinc-800/70 bg-zinc-950/50 p-5">
        <div className="space-y-3 text-sm leading-relaxed text-zinc-300">
          <ReactMarkdown
            components={{
              h2: (props) => (
                <h2 className="mt-4 text-sm font-semibold text-coral-300" {...props} />
              ),
              h3: (props) => (
                <h3 className="mt-3 text-sm font-semibold text-zinc-200" {...props} />
              ),
              p: (props) => <p className="text-zinc-300" {...props} />,
              strong: (props) => (
                <strong className="font-semibold text-zinc-100" {...props} />
              ),
              ul: (props) => (
                <ul className="ml-5 list-disc space-y-1" {...props} />
              ),
              li: (props) => <li className="text-zinc-300" {...props} />,
            }}
          >
            {markdown}
          </ReactMarkdown>
        </div>
      </div>
    );
  }

  const get = (t: SectionTitle) => sections.get(t)?.trim() ?? "";

  return (
    <div className="space-y-7">
      {/* Hero scoreboard — always first when present. */}
      {sections.has("Placar") && <ScoreboardHero body={get("Placar")} />}

      {/* Resumo Executivo */}
      {sections.has("Resumo Executivo") && (
        <SectionCard
          title="Resumo Executivo"
          icon={SECTION_META["Resumo Executivo"].icon}
          accent={SECTION_META["Resumo Executivo"].accent}
        >
          <ProseSection body={get("Resumo Executivo")} />
        </SectionCard>
      )}

      {/* Forças + Pontos de Atenção side by side on wider screens. */}
      {(sections.has("Forças") || sections.has("Pontos de Atenção")) && (
        <div className="grid gap-7 lg:grid-cols-2">
          {sections.has("Forças") && (
            <SectionCard
              title="Forças"
              icon={SECTION_META["Forças"].icon}
              accent={SECTION_META["Forças"].accent}
            >
              <CheckListSection body={get("Forças")} variant="positive" />
            </SectionCard>
          )}
          {sections.has("Pontos de Atenção") && (
            <SectionCard
              title="Pontos de Atenção"
              icon={SECTION_META["Pontos de Atenção"].icon}
              accent={SECTION_META["Pontos de Atenção"].accent}
            >
              <CheckListSection body={get("Pontos de Atenção")} variant="warning" />
            </SectionCard>
          )}
        </div>
      )}

      {/* Qualitative sections grid: 2 columns. */}
      <div className="grid gap-7 lg:grid-cols-2">
        {sections.has("Pronúncia e Inteligibilidade") && (
          <SectionCard
            title="Pronúncia e Inteligibilidade"
            icon={SECTION_META["Pronúncia e Inteligibilidade"].icon}
            accent={SECTION_META["Pronúncia e Inteligibilidade"].accent}
          >
            <ProseSection body={get("Pronúncia e Inteligibilidade")} />
          </SectionCard>
        )}
        {sections.has("Fluência e Ritmo") && (
          <SectionCard
            title="Fluência e Ritmo"
            icon={SECTION_META["Fluência e Ritmo"].icon}
            accent={SECTION_META["Fluência e Ritmo"].accent}
          >
            <ProseSection body={get("Fluência e Ritmo")} />
          </SectionCard>
        )}
        {sections.has("Gramática Oral e Estrutura") && (
          <SectionCard
            title="Gramática Oral e Estrutura"
            icon={SECTION_META["Gramática Oral e Estrutura"].icon}
            accent={SECTION_META["Gramática Oral e Estrutura"].accent}
          >
            <ProseSection body={get("Gramática Oral e Estrutura")} />
          </SectionCard>
        )}
        {sections.has("Vocabulário e Adequação") && (
          <SectionCard
            title="Vocabulário e Adequação"
            icon={SECTION_META["Vocabulário e Adequação"].icon}
            accent={SECTION_META["Vocabulário e Adequação"].accent}
          >
            <ProseSection body={get("Vocabulário e Adequação")} />
          </SectionCard>
        )}
        {sections.has("Naturalidade e Registro") && (
          <SectionCard
            title="Naturalidade e Registro"
            icon={SECTION_META["Naturalidade e Registro"].icon}
            accent={SECTION_META["Naturalidade e Registro"].accent}
          >
            <ProseSection body={get("Naturalidade e Registro")} />
          </SectionCard>
        )}
        {sections.has("Evidências do Áudio") && (
          <SectionCard
            title="Evidências do Áudio"
            icon={SECTION_META["Evidências do Áudio"].icon}
            accent={SECTION_META["Evidências do Áudio"].accent}
          >
            <CheckListSection body={get("Evidências do Áudio")} variant="positive" />
          </SectionCard>
        )}
      </div>

      {/* Plano de Melhoria — full width. */}
      {sections.has("Plano de Melhoria") && (
        <SectionCard
          title="Plano de Melhoria"
          icon={SECTION_META["Plano de Melhoria"].icon}
          accent={SECTION_META["Plano de Melhoria"].accent}
        >
          <ImprovementSection body={get("Plano de Melhoria")} />
        </SectionCard>
      )}

      {/* Veredito Final — full width, highlighted. */}
      {sections.has("Veredito Final") && (
        <SectionCard
          title="Veredito Final"
          icon={SECTION_META["Veredito Final"].icon}
          accent={SECTION_META["Veredito Final"].accent}
        >
          <div className="rounded-xl border border-coral-500/25 bg-gradient-to-br from-coral-500/5 to-zinc-950/40 px-5 py-4">
            <div className="flex gap-3">
              <ShieldQuestion className="mt-0.5 h-4 w-4 shrink-0 text-coral-400" />
              <ProseSection body={get("Veredito Final")} />
            </div>
          </div>
        </SectionCard>
      )}
    </div>
  );
}
