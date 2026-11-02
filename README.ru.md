<div align="center">

# stint-busfs

### D-Bus как виртуальная файловая система

**Смонтируйте D-Bus в точку монтирования и работайте с ним через `ls`, `cat`,
`echo` — доступно из шелла, pipe'ится, скриптуется, с DSO-контрактами и
SHA-256-цепочкой провенанса на каждой операции.**

[![Release](https://img.shields.io/github/v/release/kostyk348/stint-busfs?sort=semver&color=0ea5e9&label=release)](https://github.com/kostyk348/stint-busfs/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/kostyk348/stint-busfs/ci.yml?branch=master&label=CI)](https://github.com/kostyk348/stint-busfs/actions/workflows/ci.yml)
[![tests](https://img.shields.io/badge/tests-18%20passing-22c55e)](#разработка)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)](https://www.rust-lang.org)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](CONTRIBUTING.md)

[English](README.md) · **Русский**

</div>

---

## Что это такое?

D-Bus мощный, но невидимый из шелла. `stint-busfs` монтирует шину как файловую
систему: каждый well-known сервис становится каталогом, каждая операция — файлом:

```bash
stint-busfs mount /tmp/busfs
# в другом терминале:
ls /tmp/busfs/session
cat /tmp/busfs/session/org.freedesktop.DBus/introspect
echo "ListNames" > /tmp/busfs/session/org.freedesktop.DBus/methods
```

Часть видения **Proximated Linux**: переработка Linux IPC через принципы DSO-контрактов.

## Виртуальное дерево

```
/<шина>/<сервис>/<файл>          например /session/org.freedesktop.UPower/properties
```

| файл | что даёт `cat` | что делает запись |
|---|---|---|
| `introspect` | XML интроспекции D-Bus | — |
| `properties` | `Properties.GetAll`, в виде `ключ = значение` | — |
| `interfaces` | имена интерфейсов из интроспекции | — |
| `signals` | имена сигналов из интроспекции | — |
| `methods` | подсказка | `echo "ИмяМетода" > methods` вызывает метод |

Шина недоступна? Монтирование всё равно работает в **demo-режиме**: то же
дерево со статическим списком сервисов и честным содержимым `demo (no bus)`.

## Архитектура

```
┌─ fuser (FUSE3, многопоточный) ──┐   ┌─ мост zbus (чистый Rust) ─┐
│ ls | cat | echo   → FUSE-операции│  │  Introspect / GetAll /    │
│        ↓                        │   │  ListNames / вызовы методов│
│  vtree (путь → узел, иноды)     │   └───────────────────────────┘
│        ↓                        │
│  контракты + цепочка провенанса │
└─────────────────────────────────┘
```

- **FUSE3 многопоточный** через `fuser` — без link-зависимости от libfuse, монтирование чистое Rust (через `fusermount3`).
- **Настоящий D-Bus** через `zbus` — чистая Rust-реализация, без `libdbus-1` и OpenSSL.
- **DSO-контракты** на каждой операции: pre-условия DENY, post-условия FALLBACK, нарушения считаются и логируются.
- **SHA-256 hash-chain провенанс** — запись на каждую FUSE-операцию, проверяется повторным проходом цепочки; подделка обнаруживается.

## Что изменил порт на Rust (v0.2)

Прототип был на C с libfuse3 + libdbus + OpenSSL. Порт сохраняет семантику
и обновляет внутренности:

| | C-прототип | Rust 0.2 |
|---|---|---|
| FUSE | libfuse3, ручная таблица операций | `fuser`, чистое Rust-монтирование |
| D-Bus | C API libdbus-1 | `zbus` (чистый Rust) |
| хэширование | OpenSSL `SHA256` | крейт `sha2` |
| список сервисов | 7 захардкоженных имён | живой `ListNames`, фолбэк — статический список |
| файл `properties` | заглушка (XML интроспекции) | настоящий `Properties.GetAll`, отсортированный `ключ = значение` |
| файлы `interfaces`/`signals` | — | разбираются из живой интроспекции |
| контракты | заглушки (всегда проходят) | проверка границ размера, путей, счётчик нарушений |
| модель инод | неявная | стабильные иноды для lookup/getattr/readdir |
| тесты | нет | 18 (дерево, контракты, провенанс с детекцией подделки, demo-режим) |

## Установка

```bash
cargo install --path .
```

Требования в рантайме: ядро Linux с FUSE (`fusermount3` есть в любом дистрибутиве
с установленным `fuse3`). Никаких C-библиотек для сборки и линковки.

## Использование

```bash
stint-busfs mount <точка_монтирования> [--system] [--no-contracts] [--no-provenance]
stint-busfs list [--system]          # сервисы шины без монтирования
stint-busfs --version
```

## Разработка

```bash
cargo fmt --check                          # форматирование
cargo clippy --all-targets -- -D warnings  # линты, нулевая толерантность
cargo test                                 # 18 тестов, шина и монтирование не нужны
cargo build --release
```

Всё, кроме тонкого FUSE-клея, тестируется офлайн: разбор путей, виртуальное
дерево, эскалация контрактов и цепочка провенанса (включая детекцию подделки) —
обычные юнит-тесты; CI не требует ни шины, ни `/dev/fuse`.

См. [CONTRIBUTING.md](CONTRIBUTING.md) и [CHANGELOG.md](CHANGELOG.md).

## Оговорки

* Вызовы методов пока передают без аргументов (как в прототипе); типизированный
  синтаксис аргументов в файле `methods` — следующий шаг.
* `properties` использует первый интерфейс из интроспекции; дамп свойств всех
  интерфейсов — будущая работа.
* В дереве сервисов только well-known имена — уникальные `:1.x`-подключения
  отфильтрованы намеренно.
