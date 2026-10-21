```
                 |     |     |
                 |     |     |
                  \    |    /
                   \   |   /
                    \  |  /                    .--.
               ======\(•)/======              (    )
                      \|/                       )  )   ~ lua crescente
                       |                        (  (
                       |                         `--'
                       |
                       |          S H I V A   D E C O M P I L E R
                  ~~~~~~~~~~~~
                   trishula

        coleta . emula . reverte   —   analise segura de amostras
```

# ShivaDecompiler

Ferramenta web, escrita em **Rust**, para **coleta e análise segura de amostras**
voltada a pesquisa de segurança / reversing. Implementação **original** (não é um
port nem cópia de nenhum gerenciador de downloads existente), com interface
inspirada no fluxo de um gerenciador de downloads clássico.

A ideia: baixar arquivos com **velocidade** (inclusive da rede `.onion`), guardar
tudo num cofre isolado, e a partir daí **emular/detonar** binários num Docker
remoto e **reverter** com um módulo de disassembly — sem nada rodar na máquina do
operador.

## Funcionalidades

### Coleta (Downloads)
- Fila de downloads com **Novo download**: várias URLs de uma vez.
- **Download segmentado** (múltiplas conexões por arquivo, via HTTP Range) para
  maximizar velocidade — o recurso de performance no estilo do aria2/AriaNg.
- **Velocidade em tempo real** por tarefa e **velocidade global** no topo.
- **Tor automático**: URLs `.onion` são roteadas pelo proxy SOCKS5
  (`socks5h://127.0.0.1:9050`); clearnet vai direto.
- Estados **Baixando / Aguardando / Parados** com contadores.
- **Detalhe da tarefa** (painel lateral): URL, estado, conexões, progresso,
  bytes, velocidade, `sha256`, caminho em disco, erro.
- **Remover** (com **cancelamento real** da transferência em andamento),
  **Repetir com erro**, **Limpar concluídos**.
- Cada arquivo é gravado como **bytes opacos** e tem o **SHA-256** calculado.
  Nada é executado localmente.

### Terminal do cofre
- Navegação estilo terminal **enjaulada** na pasta de amostras:
  `ls`, `cd <dir>`, `pwd`, `cat <arquivo>`, `clear`, `help`.
- **Sem execução de comando do SO** — é um navegador de arquivos seguro, com
  proteção contra traversal (`../` não escapa da raiz).

### Sandbox (detonação remota)
- Binários são detonados num **Docker remoto** (VPS/lab dedicado), **nunca** na
  máquina do operador.
- Container endurecido: `--network none` por padrão, rootfs read-only,
  `cap_drop ALL`, usuário não-root, limites de memória/pids/tempo.
- Executor plugável (`StubExecutor` até ligar o Docker-over-TLS / runner SSH).

### Disassembly Packages
- Submete um binário a vários backends de disassembly/decompilação lado a lado
  (asm, pseudo-C, reversing assistido por LLM) — ideia parecida com o dogbolt.
- Backends plugáveis via trait `DisasmBackend`.

### Sistema
- **Status do engine**: ativos/aguardando/concluídos/falhas, velocidade total,
  total baixado, Tor, sandbox.
- **Configurações**: intervalo de atualização e conexões padrão (persistem no
  navegador).
- **Idioma PT/EN**, tema branco/cinza, fonte Helvetica, botões quadrados.

## Arquitetura

```
crates/
  shiva-core/    engine headless
    download.rs  fila + download segmentado + Tor + cancelamento + stats
    vault.rs     navegador de arquivos enjaulado (terminal)
    sandbox.rs   contrato de detonação em Docker remoto
    disasm.rs    módulo Disassembly Packages
  shiva-server/  servidor Axum: serve o frontend + API JSON
web/             frontend SPA (HTML/CSS/JS, sem build step)
```

## Rodando

```bash
cargo run -p shiva-server
# abre http://127.0.0.1:8777
```

> No Windows, o toolchain `x86_64-pc-windows-gnu` precisa do MinGW-w64 no PATH
> (para `dlltool`/`gcc`/`ld`). Ex.: WinLibs.

### Variáveis de ambiente

| Var | Padrão | Descrição |
|-----|--------|-----------|
| `SHIVA_BIND` | `127.0.0.1:8777` | Endereço de bind |
| `SHIVA_DOWNLOAD_DIR` | `./downloads` | Cofre de amostras |
| `SHIVA_TOR_PROXY` | `socks5h://127.0.0.1:9050` | Proxy Tor (para `.onion`) |
| `SHIVA_DOCKER_HOST` | — | Docker da VPS de análise (TLS recomendado) |
| `SHIVA_WEB_DIR` | `./web` | Diretório do frontend |

## API

| Método | Rota | Descrição |
|--------|------|-----------|
| `GET`  | `/api/health` | Status |
| `GET`  | `/api/stats` | Agregados (contagens, velocidade total) |
| `GET`  | `/api/downloads` | Lista tarefas |
| `POST` | `/api/downloads` | `{ "url", "connections?", "out_name?" }` enfileira |
| `DELETE` | `/api/downloads/{id}` | Remove/cancela |
| `GET`  | `/api/fs?path=` | Lista diretório do cofre |
| `GET`  | `/api/fs/cat?path=` | Lê (head) um arquivo do cofre |
| `POST` | `/api/sandbox/detonate` | Detonação remota |
| `POST` | `/api/disasm` | Executa backends de análise |

## Segurança

- Binários coletados **nunca** rodam no host do operador.
- `.onion` sempre via Tor (DNS resolvido no proxy, `socks5h`).
- Terminal do cofre é enjaulado, sem execução de comandos.
- Container de detonação isolado e sem rede por padrão.

## Estado atual

Base funcional. O executor de sandbox e os backends de disassembly são *stubs*
registráveis; **pausa/retomada** de download ainda não é resumível. Próximos
passos: ligar o Docker-over-TLS na VPS, plugar as engines de reversing, e o LLM.

## Licença

AGPL-3.0-or-later.
