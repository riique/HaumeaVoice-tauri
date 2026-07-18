//! Versioned Gemini prompts (transcription / refinement / pronunciation).
//!
//! Bump the version constants when the prompt text changes so history/debug
//! snapshots can tell which instruction the model saw.

/// Transcription system+user instruction (audio → text only).
pub const TRANSCRIBE_PROMPT_VERSION: &str = "transcribe-v1-2026-07";

/// Refinement instruction (audio + draft → improved text).
pub const REFINE_PROMPT_VERSION: &str = "refine-v1-2026-07";

/// Precise mode: audio primary + Whisper hypothesis + vocabulary.
pub const PRECISE_PROMPT_VERSION: &str = "precise-v1-2026-07";

/// UltraPrecise: audio + Whisper raw + sanitized text + vocabulary.
pub const ULTRAPRECISE_PROMPT_VERSION: &str = "ultraprecise-v1-2026-07";

/// Pronunciation evaluation instruction (unchanged product contract).
pub const PRONUNCIATION_PROMPT_VERSION: &str = "pronunciation-v1-cefr";

/// Full prompt for pure audio transcription (pt-BR, code-switching, no invention).
pub fn transcription_prompt() -> &'static str {
    r#"Você é um motor de transcrição de áudio. Transcreva o que foi falado com fidelidade.

Idioma e registro:
- Priorize português do Brasil.
- Preserve code-switching (mistura de português e inglês ou outros idiomas) exatamente como falado.
- Não traduza. Não normalize sotaques para outra língua.

Fidelidade (obrigatório):
- NÃO resuma, NÃO omita, NÃO invente palavras ou trechos.
- NÃO acrescente introduções, títulos, notas, legendas ou comentários.
- Preserve números, versões (ex.: 1.0.3), comandos, caminhos de arquivo, URLs, nomes próprios, marcas e jargão técnico na grafia mais provável pelo áudio.
- Preserve hesitações só se forem claramente ditas como palavras ("né", "tipo", "hmm"); não invente ruído.

Saída:
- Devolva APENAS o texto da transcrição, limpo e legível.
- Use pontuação natural quando o áudio sustentar.
- Se o áudio estiver inaudível ou vazio, devolva uma string vazia (nada mais)."#
}

/// FastAccurate STT prompt with optional strict glossary + content-type hint.
pub fn fast_accurate_transcription_prompt(glossary_block: &str, content_note: &str) -> String {
    let vocab = if glossary_block.trim().is_empty() {
        "(nenhum termo cadastrado)".to_string()
    } else {
        glossary_block.trim().to_string()
    };
    let content = if content_note.is_empty() {
        String::new()
    } else {
        format!("\nTipo de conteúdo (heurística): {content_note}\n")
    };
    format!(
        r#"{base}
{content}
Glossário do usuário (use a grafia canônica quando o áudio encaixar; [LITERAL] = rígido — nunca reescreva):
{vocab}

Regras extras:
- Não “corrija” nomes de produtos, arquivos, funções ou identificadores para formas mais comuns.
- Não altere caminhos, versões, comandos ou URLs.
- Se o tipo for programação, preserve código e identificadores; se estudo, preserve termos técnicos.
"#,
        base = transcription_prompt(),
        content = content,
        vocab = vocab
    )
}

/// Prompt for refining a draft transcription against the source audio.
pub fn refinement_prompt(draft: &str) -> String {
    format!(
        r#"Você é um revisor de transcrição por voz. Você recebe (1) o áudio original e (2) um rascunho acústico.

Tarefa:
- Ouça o áudio como fonte principal.
- Use o rascunho só como apoio.
- Produza UM texto final fiel ao que foi dito, em português do Brasil, preservando code-switching e termos técnicos.

Regras:
- NÃO resuma, NÃO invente, NÃO traduza.
- Corrija erros óbvios do rascunho quando o áudio for claro.
- Preserve números, versões, comandos, caminhos, nomes e marcas.
- NÃO inclua introduções, notas, aspas exteriores ou explicações.
- Saída: somente o texto final.

Rascunho acústico:
"""
{draft}
"""
"#,
        draft = draft
    )
}

/// Precise mode prompt: audio is ground truth; Whisper is a hypothesis.
pub fn precise_refinement_prompt(
    whisper_hypothesis: &str,
    glossary_block: &str,
    content_note: &str,
) -> String {
    let vocab = if glossary_block.trim().is_empty() {
        "(nenhum termo cadastrado)".to_string()
    } else {
        glossary_block.trim().to_string()
    };
    let content = if content_note.is_empty() {
        String::new()
    } else {
        format!("\nTipo de conteúdo (heurística sobre a hipótese): {content_note}\n")
    };

    format!(
        r#"Você é o revisor final de uma digitação por voz de alta precisão.

Entradas:
1) O ÁUDIO original (fonte principal e autoritativa).
2) Uma HIPÓTESE do Whisper (rascunho acústico — pode conter erros).
3) Um glossário opcional de termos do usuário (canônico, categoria, aliases; [LITERAL] = rígido).
{content}
Idioma:
- Priorize português do Brasil.
- Preserve code-switching e jargão técnico como falados.
- NÃO traduza.

Regras de fidelidade:
- O áudio manda. A hipótese do Whisper é só apoio.
- NÃO resuma, NÃO invente, NÃO omita o que o áudio sustenta.
- Corrija a hipótese quando o áudio for mais claro.
- Preserve números, versões, comandos, caminhos, URLs, nomes e marcas.
- Se um termo do glossário encaixar claramente no áudio, use a grafia canônica.
- Termos [LITERAL] nunca devem ser reescritos com outra grafia.
- NÃO force termos do glossário onde não pertencem.
- Se o tipo for programação, preserve código/identificadores; se estudo, preserve terminologia.
- Saída: APENAS o texto final, sem títulos, notas ou aspas exteriores.

Hipótese Whisper:
"""
{hypothesis}
"""

Glossário do usuário:
{vocab}
"#,
        content = content,
        hypothesis = whisper_hypothesis,
        vocab = vocab
    )
}

/// CEFR oral-proficiency rubric (existing product behaviour).
pub fn pronunciation_prompt(transcript: &str) -> String {
    format!(
        "Analise o áudio como um avaliador internacional de proficiência oral e \
comunicação.\n\n\
Responda em português do Brasil, em Markdown, sem introduções fora da \
estrutura pedida.\n\n\
Objetivo da avaliação:\n\
- medir inteligibilidade, pronúncia, fluência, ritmo, entonação, gramática \
oral, vocabulário, coesão, naturalidade, segurança e adequação ao contexto;\n\
- dar uma nota geral;\n\
- classificar o desempenho na escala internacional CEFR (A1, A2, B1, B2, C1, C2);\n\
- indicar o quão próximo o desempenho está de uma fala nativa, sem exagerar a \
conclusão.\n\n\
Escalas obrigatórias:\n\
- Nota geral: 0 a 10, com 1 casa decimal.\n\
- CEFR estimado: A1, A2, B1, B2, C1 ou C2.\n\
- Referência internacional de fala: Básico em desenvolvimento, Intermediário \
funcional, Fluente profissional, Quase nativo ou Nativo.\n\
- Proximidade de fala nativa: 0 a 100.\n\
- Confiança da avaliação: baixa, média ou alta.\n\n\
Regras:\n\
1. Priorize o áudio como fonte principal.\n\
2. Use a transcrição apenas como apoio, porque ela pode ter sido limpa \
automaticamente.\n\
3. Se o áudio estiver curto demais, ruim, com ruído forte, silêncio ou material \
insuficiente, diga isso explicitamente e reduza a confiança.\n\
4. Não invente palavras, contexto, sotaque, nacionalidade ou nível que o áudio \
não sustente.\n\
5. A avaliação deve equilibrar pontos fortes e pontos fracos.\n\
6. Diferencie com rigor:\n\
   - fluência funcional;\n\
   - fluência avançada;\n\
   - quase nativo;\n\
   - nativo.\n\
7. Só use \"Nativo\" se houver evidência muito forte e consistente. Na dúvida, \
use uma classificação abaixo.\n\
8. Se o áudio estiver em outro idioma, avalie no idioma falado, mas mantenha a \
resposta em português.\n\
9. Quando citar evidências, prefira trechos curtos ou paráfrases claramente \
reconhecíveis do próprio áudio.\n\
10. Seja específico, direto, técnico e construtivo.\n\n\
Estrutura obrigatória da resposta:\n\
## Resumo Executivo\n\
Escreva de 2 a 4 frases com o diagnóstico principal.\n\n\
## Placar\n\
- Nota geral: X/10\n\
- CEFR estimado: ...\n\
- Referência internacional de fala: ...\n\
- Proximidade de fala nativa: X/100\n\
- Confiança da avaliação: ...\n\n\
## Forças\n\
Liste de 3 a 5 pontos fortes objetivos.\n\n\
## Pontos de Atenção\n\
Liste de 3 a 5 pontos que mais limitam a performance.\n\n\
## Pronúncia e Inteligibilidade\n\
Avalie articulação, sons, sotaque, compreensão e inteligibilidade geral.\n\n\
## Fluência e Ritmo\n\
Avalie pausas, velocidade, hesitações, continuidade e naturalidade do fluxo.\n\n\
## Gramática Oral e Estrutura\n\
Avalie construção de frases, concordância, precisão e organização das ideias \
ao falar.\n\n\
## Vocabulário e Adequação\n\
Avalie variedade lexical, precisão vocabular, repetições e adequação ao \
contexto.\n\n\
## Naturalidade e Registro\n\
Avalie segurança, espontaneidade, entonação, registro e o quanto a fala soa \
natural.\n\n\
## Evidências do Áudio\n\
Liste de 3 a 5 evidências curtas do áudio que sustentam a avaliação.\n\n\
## Plano de Melhoria\n\
- Traga 5 ações práticas e priorizadas.\n\
- Traga 3 exercícios específicos para subir um nível.\n\n\
## Veredito Final\n\
Feche com 1 parágrafo explicando por que essa foi a nota geral, qual o nível \
internacional mais provável e o que falta para chegar ao próximo patamar.\n\n\
Transcrição de referência (use apenas como apoio):\n\"\"\"\n{}\n\"\"\"",
        transcript
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_stable_markers() {
        assert!(TRANSCRIBE_PROMPT_VERSION.starts_with("transcribe-"));
        assert!(REFINE_PROMPT_VERSION.starts_with("refine-"));
        assert!(PRONUNCIATION_PROMPT_VERSION.starts_with("pronunciation-"));
    }

    #[test]
    fn transcription_prompt_forbids_invention() {
        let p = transcription_prompt();
        assert!(p.contains("invente"));
        assert!(p.contains("português"));
        assert!(p.contains("code-switching"));
    }

    #[test]
    fn refine_includes_draft() {
        let p = refinement_prompt("rascunho xyz");
        assert!(p.contains("rascunho xyz"));
    }

    #[test]
    fn precise_prompt_audio_primary_and_vocab() {
        let p =
            precise_refinement_prompt("hipótese whisper", "- Haumea [application]", "programming");
        assert!(p.contains("fonte principal") || p.contains("ÁUDIO"));
        assert!(p.contains("hipótese whisper"));
        assert!(p.contains("Haumea"));
        assert!(p.contains("programming"));
        assert!(PRECISE_PROMPT_VERSION.starts_with("precise-"));
    }

    #[test]
    fn ultraprecise_prompt_has_both_texts() {
        let p = ultraprecise_refinement_prompt("w raw", "s clean", "- Foo [file]", "programming");
        assert!(p.contains("w raw"));
        assert!(p.contains("s clean"));
        assert!(p.contains("Foo"));
        assert!(ULTRAPRECISE_PROMPT_VERSION.starts_with("ultraprecise-"));
    }
}

/// UltraPrecise prompt: audio primary; Whisper raw + sanitized as supports.
pub fn ultraprecise_refinement_prompt(
    whisper_raw: &str,
    sanitized: &str,
    glossary_block: &str,
    content_note: &str,
) -> String {
    let vocab = if glossary_block.trim().is_empty() {
        "(nenhum termo cadastrado)".to_string()
    } else {
        glossary_block.trim().to_string()
    };
    let content = if content_note.is_empty() {
        String::new()
    } else {
        format!("\nTipo de conteúdo (heurística): {content_note}\n")
    };

    format!(
        r#"Você é o revisor final ultrapreciso de digitação por voz.

Entradas:
1) ÁUDIO original (fonte principal).
2) Whisper bruto (hipótese acústica).
3) Texto já sanitizado (limpeza ortográfica — pode ter errado literais).
4) Glossário do usuário ([LITERAL] = rígido).
{content}
Regras:
- O áudio manda sobre qualquer texto.
- Use o sanitizado como base fluida, mas restaure literais/caminhos/comandos se o áudio + Whisper bruto forem mais fiéis.
- NÃO resuma, NÃO invente, NÃO traduza.
- Termos [LITERAL]: grafia canônica obrigatória quando o áudio encaixar.
- Saída: APENAS o texto final (sem JSON, sem notas).

Whisper bruto:
"""
{whisper}
"""

Texto sanitizado:
"""
{sanitized}
"""

Glossário:
{vocab}
"#,
        content = content,
        whisper = whisper_raw,
        sanitized = sanitized,
        vocab = vocab
    )
}
