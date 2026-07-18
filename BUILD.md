# Build do Projeto Haumea Voice

Este documento descreve como realizar o build de produção do aplicativo desktop Tauri.

## Requisitos

1. **Node.js** (npm) instalado.
2. **Rust** instalado (através do [rustup](https://rustup.rs/)).

## Executando o Build

### Cenário 1: Rust configurado no PATH
Execute o comando abaixo na raiz do projeto:

```bash
npm run tauri build
```

### Cenário 2: Rust instalado mas fora do PATH (Windows)
Se o compilador do Rust (`cargo`) não estiver acessível globalmente, execute via PowerShell adicionando temporariamente o diretório padrão de instalação do Rust ao `PATH`:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
npm run tauri build
```

## Artefato Gerado

Após a finalização do processo, o executável compilado estará disponível em:

```
src-tauri/target/release/haumea-voice.exe
```
