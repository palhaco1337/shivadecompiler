# ShivaDecompiler

Ferramenta web, escrita em **Rust**, para **coleta e análise segura de amostras**
voltada a pesquisa de segurança / reversing. Implementação original (não é um
port de nenhum gerenciador de downloads existente).

## Módulos

| Módulo | O que faz |
|--------|-----------|
| **Coleta** | Baixa recursos como *bytes opacos*. URLs `.onion` são roteadas pelo Tor (SOCKS5). Nada é executado localmente — só grava em disco e calcula o SHA-256. |
| **Sandbox** | Detona binários num **Docker isolado e remoto** (VPS/lab dedicado), nunca na máquina do operador. Container endurecido: sem rede, rootfs read-only, caps dropadas, usuário não-root, limites de CPU/memória/tempo. |
| **Disassembly Packages** | Painel lateral que submete um binário a vários backends de disassembly/decompilação lado a lado (asm, pseudo-C, reversing assistido por LLM). |

## Arquitetura

```
crates/
  shiva-core/    engine headless (download, sandbox, disasm)
  shiva-server/  servidor Axum: serve o frontend + API JSON
web/             frontend SPA (HTML/CSS/JS, sem build step)
```

## Rodando

```bash
# precisa do Tor rodando localmente p/ .onion (porta SOCKS 9050)
cargo run -p shiva-server
# abre http://127.0.0.1:8777
```

### Variáveis de ambiente

| Var | Padrão | Descrição |
|-----|--------|-----------|
| `SHIVA_BIND` | `127.0.0.1:8777` | Endereço de bind |
| `SHIVA_DOWNLOAD_DIR` | `./downloads` | Onde as amostras são gravadas |
| `SHIVA_TOR_PROXY` | `socks5h://127.0.0.1:9050` | Proxy Tor |
| `SHIVA_DOCKER_HOST` | — | Endpoint Docker da VPS de análise (TLS recomendado) |
| `SHIVA_WEB_DIR` | `./web` | Diretório do frontend |

## API

| Método | Rota | Descrição |
|--------|------|-----------|
| `GET`  | `/api/health` | Status |
| `GET`  | `/api/downloads` | Lista tarefas |
| `POST` | `/api/downloads` | `{ "url": "..." }` enfileira |
| `POST` | `/api/sandbox/detonate` | Detonação remota |
| `POST` | `/api/disasm` | Executa backends de análise |

## Estado atual

Esqueleto funcional. O executor de sandbox (`StubExecutor`) e os backends de
disassembly são *stubs* registráveis — plugar o cliente Docker-over-TLS (ex.
`bollard`) ou runner SSH na VPS, e as engines de reversing, é o próximo passo.

## Segurança

- Binários coletados **nunca** rodam no host do operador.
- `.onion` sempre via Tor (DNS resolvido no proxy, `socks5h`).
- Container de detonação isolado e sem rede por padrão.

## Licença

AGPL-3.0-or-later.
