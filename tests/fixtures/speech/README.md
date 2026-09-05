# Fala sintética local

`oi.wav`, `sim.wav` e `frase-curta.wav` foram gerados com Windows SAPI (`generate.ps1`), sem internet e sem gravações do usuário. PCM mono, 16 bits, 16 kHz. A frase é “Bom dia, tudo bem?”.

O teste Rust reduz o volume e acrescenta pausas de 15 segundos dos dois lados. Estes exemplos verificam que falas curtas não são descartadas pela proporção de silêncio. Não representam uma validação física de microfone, sotaques ou ruído ambiente.
