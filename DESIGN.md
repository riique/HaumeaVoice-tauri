---
name: Haumea Voice
description: Quiet Windows utility for fast, recoverable dictation.
colors:
  canvas: "#f7f7f4"
  sidebar: "#f1f1ed"
  surface: "#ffffff"
  ink: "#181816"
  muted: "#6f706a"
  line: "#deded8"
  line-strong: "#c7c8c0"
  control: "#1d1d1b"
  nav-active: "#dfdfd9"
  success: "#25613f"
  danger: "#9f2720"
  danger-soft: "#fff1ef"
  gadget: "#171716"
  gadget-muted: "#c8c8c1"
typography:
  headline:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    letterSpacing: "-0.015em"
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.538
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 500
  mono:
    fontFamily: "Cascadia Mono, Cascadia Code, Consolas, monospace"
    fontSize: "11px"
    fontWeight: 400
rounded:
  compact: "5px"
  tag: "6px"
  kbd: "7px"
  sm: "8px"
  control: "10px"
  menu: "12px"
  surface: "14px"
  pill: "9999px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "5": "20px"
  "6": "24px"
  "8": "32px"
  "10": "40px"
  "12": "48px"
components:
  button-primary:
    backgroundColor: "{colors.control}"
    textColor: "{colors.surface}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 16px"
    height: "40px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 16px"
    height: "40px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 16px"
    height: "40px"
  button-danger:
    backgroundColor: "transparent"
    textColor: "{colors.danger}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 16px"
    height: "40px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 14px"
    height: "40px"
  surface:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    rounded: "{rounded.surface}"
  gadget:
    backgroundColor: "{colors.gadget}"
    textColor: "{colors.surface}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    height: "44px"
---

# Design System: Haumea Voice

## Overview

**Creative North Star: "Quiet by default"**

Haumea Voice é um utilitário Windows que deve desaparecer no fluxo de trabalho. A janela principal organiza capacidade técnica com densidade de operação, mas sem se apresentar como dashboard: hierarquia vem de espaço, tipografia, divisores finos e divulgação progressiva.

O mundo visual é light-first e quase monocromático. Warm white, branco, near-black e cinzas neutros sustentam toda a interface; cor aparece apenas quando um estado real precisa ser reconhecido. A exceção memorável é o Haumea Bar, uma pílula preta compacta que se expande conforme gravação, processamento, sucesso ou falha exigem informação.

**Key Characteristics:**

- Utilitário desktop-native, silencioso e orientado à tarefa.
- Densidade Operate com controles compactos e áreas de trabalho legíveis.
- Superfícies flat-by-default, separadas por tom e hairlines.
- Cor reservada a estados semânticos.
- Haumea Bar preto como assinatura recorrente e responsiva ao estado.

## Colors

A paleta combina papéis quentes e neutros com tinta near-black; nenhuma cor decorativa compete com o trabalho.

### Primary

- **Workhorse Black:** controles primários, foco e ações de alta prioridade.

### Tertiary

- **Semantic Green:** confirmação e disponibilidade reais.
- **Semantic Red:** gravação, falha e ações destrutivas.
- **Soft Error Wash:** fundo de mensagens de erro sem transformar a página em alerta.

### Neutral

- **Warm Canvas:** fundo contínuo da janela principal.
- **Quiet Sidebar:** plano de navegação sutilmente separado do canvas.
- **Working White:** campos, menus e superfícies de conteúdo.
- **Near-black Ink:** texto principal e ícones de maior ênfase.
- **Muted Graphite:** descrições, metadados e navegação inativa.
- **Hairline Gray:** bordas, regras e divisores; a versão forte aparece em hover ou em limites que precisam de definição adicional.
- **Gadget Black:** corpo da Haumea Bar, independente do canvas claro.

### Named Rules

**The Semantic Color Rule.** Fora dos neutros, cor só comunica um estado real; nunca funciona como decoração, branding ornamental ou preenchimento de seção.

## Typography

**Display Font:** Segoe UI Variable Text, com Segoe UI e system-ui como fallback

**Body Font:** Segoe UI Variable Text, com Segoe UI e system-ui como fallback
**Label/Mono Font:** Cascadia Mono, com Cascadia Code e Consolas como fallback

**Character:** Uma única sans de sistema trabalha em todos os níveis e mantém o aplicativo familiar no Windows. Peso, tamanho e espaçamento criam hierarquia sem depender de fontes de personalidade ou contrastes editoriais.

### Hierarchy

- **Headline** (600, 28px, 1.25, -0.025em): título principal de cada página.
- **Title** (600, 17px, -0.015em): pipeline ativa e outros destaques locais.
- **Section title** (600, 14px, 20px): entrada de seções operacionais.
- **Body** (400, 13px, 20px): descrições, linhas de dados e controles; descrições longas ficam em aproximadamente 68–72ch.
- **Label** (500, 11px): metadados, atalhos e informação compacta.
- **Mono** (400, 11px): caminhos, modelos, temporizadores e valores técnicos.

### Named Rules

**The Workhorse Type Rule.** Segoe faz o trabalho inteiro; use mono apenas quando a estrutura técnica ou tabular do valor melhora a leitura.

## Layout

A janela é uma shell fixa com sidebar de 216px e conteúdo rolável. O conteúdo central usa largura máxima de 1260px, padding lateral de 28–48px conforme a largura e padding superior de 56px para conviver com a title bar transparente.

O ritmo parte de incrementos de 4px, concentrando-se em 8px, 12px, 16px, 20px, 24px, 32px e 40px. A densidade é deliberadamente operacional: linhas de preferência têm altura mínima de 76px, botões e campos padrão têm 40px, e listas separam itens por regras em vez de cartões independentes.

Em 1180px, a sidebar reduz de 216px para 76px e esconde labels; a navegação interna de Configurações vira faixa horizontal. Grids específicos colapsam em 980px, 860px, 850px e 820px conforme o conteúdo exige. Essas mudanças preservam a mesma hierarquia, sem criar uma identidade mobile paralela.

## Elevation & Depth

O sistema é flat-by-default. Canvas, sidebar e superfície branca criam profundidade por diferença tonal, bordas de 1px e divisores; sombras ficam restritas a elementos que realmente flutuam, como menus e a Haumea Bar. Teclas e o knob do toggle recebem microelevação tátil, não decoração ambiente.

### Shadow Vocabulary

- **Floating gadget:** sombra ampla em duas camadas para separar a Haumea Bar de qualquer aplicativo sob o overlay.
- **Menu:** sombra difusa somente enquanto o menu está aberto.
- **Keycap:** linha inferior de 1px para sugerir uma tecla física.
- **Toggle knob:** sombra curta para separar o knob branco do trilho.

### Named Rules

**The Flat-by-Default Rule.** Se uma superfície não flutua nem responde fisicamente ao usuário, use tom, borda ou divisor antes de considerar sombra.

## Shapes

Controles usam cantos compactos entre 8px e 10px; menus usam 12px e superfícies principais 14px. Tags pequenas podem usar 5–7px. A geometria permanece suavemente arredondada, nunca inflada: pílulas completas são reservadas ao Haumea Bar, toggles, indicadores e ações circulares dentro do gadget.

Bordas são hairlines neutras. Containers extensos podem usar apenas bordas horizontais e divisores quando um cartão fechado adicionaria peso desnecessário.

## Components

### Buttons

- **Shape:** 10px e 40px de altura por padrão; 8px e 32px no tamanho compacto.
- **Primary:** fundo Workhorse Black, texto branco, peso 500 e padding horizontal de 16px.
- **Secondary:** superfície branca com hairline; hover reforça a borda e acrescenta um wash neutro.
- **Ghost:** transparente em repouso, com wash neutro apenas em hover.
- **Danger:** texto Semantic Red em repouso e Soft Error Wash em hover.
- **Focus / Disabled:** outline near-black de 2px com offset de 2px; disabled reduz opacidade e remove a affordance de clique.

### Inputs / Fields

- **Style:** superfície branca, hairline, cantos de 10px, altura de 40px e padding horizontal de 14px.
- **Hover / Focus:** borda mais forte no hover; no foco, borda média e ring near-black com 10% de opacidade.
- **Disabled:** fundo neutral baixo, texto atenuado e cursor de indisponibilidade.

### Navigation

- **Primary sidebar:** item de 40px com cantos de 10px; inativo usa Muted Graphite e wash no hover, ativo usa Nav Active e Near-black Ink.
- **Settings navigation:** mantém a mesma lógica em item de 9px e passa para navegação horizontal abaixo de 1180px.

### Cards / Containers

- **Corner Style:** 14px em superfícies fechadas.
- **Background:** Working White ou Quiet Sidebar para agrupamentos sutis.
- **Shadow Strategy:** sem sombra em repouso.
- **Border:** hairline; listas internas usam divisores.
- **Internal Padding:** definido pelo conteúdo, normalmente 20–24px.

### Toggle

O trilho tem 40×24px e forma de pílula; desligado usa cinza médio e ligado usa near-black. O knob branco de 16px move 16px em 150ms e recebe sombra curta.

### Keyboard Shortcut

Tecla compacta com fundo Warm Canvas, borda, raio de 7px, label de 11px e uma linha inferior de 1px. Combinações usam sinais de adição discretos, nunca uma caixa única para toda a sequência.

### Haumea Bar

A assinatura do sistema é uma pílula preta de 44px de altura. O estado idle pode medir apenas 44px ou 72px; estados informativos expandem horizontalmente conforme o conteúdo. O waveform branco responde ao RMS real, vermelho aparece durante gravação, e os estados entram em 180ms com easing de desaceleração. A barra não ganha chrome, cabeçalho, gradiente ou painel auxiliar.

## Do's and Don'ts

### Do:

- **Do** mantenha a maior parte de cada tela em Warm Canvas, Working White, near-black e cinzas neutros.
- **Do** agrupe informação por espaço, hairlines e divulgação progressiva antes de criar novos containers.
- **Do** preserve controles compactos, foco visível, estados disabled/loading e hit targets adequados.
- **Do** deixe o Haumea Bar expandir somente quando o estado exige texto, waveform, recuperação ou confirmação.
- **Do** use cor apenas para sucesso, gravação, aviso, erro ou destruição reais.

### Don't:

- **Don't** reintroduza laranja, glow, gradientes ou uma estética dark dominante.
- **Don't** transforme a home ou Configurações em dashboard de métricas, mosaico de cards ou showcase de capacidades.
- **Don't** use sombra em superfícies estáticas quando tom, borda ou divisor resolve a hierarquia.
- **Don't** aplique pílulas indiscriminadamente; a silhueta completa pertence principalmente ao gadget e a controles binários.
- **Don't** copie logo, assets, tipografia ou branding de Wispr Flow ou de qualquer produto externo.
