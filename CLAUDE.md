# LiteRay — лёгкий Windows-клиент для Xray

Tauri 2 (Rust backend + Svelte 5 UI). Клиент — тонкая оболочка над `xray.exe`: генерирует JSON-конфиг, запускает ядро дочерним процессом и опрашивает его метрики. Всё сетевое (TUN через wintun, маршрутизация по процессам, DNS) делает сам Xray.

## Команды

- `npm run fetch-core` — скачать xray.exe, wintun.dll, geoip/geosite в `src-tauri/resources/xray` (с проверкой SHA-256). Нужно один раз после клонирования.
- `npx tauri dev` — запуск в dev-режиме.
- `cd src-tauri; cargo test -- --include-ignored --test-threads=1` (от администратора) — плюс интеграционные: рестарты TUN-адаптера и восстановление системного прокси. В один поток, потому что штатная остановка xray временно подключается к его консоли (`AttachConsole` действует на весь процесс).
- `cd src-tauri; cargo test` — unit-тесты. Тесты `config_builder` прогоняют каждый конфиг через `xray run -test`; snapshot-ы в `src-tauri/src/xray/snapshots` (insta, обновить: `$env:INSTA_UPDATE="always"`).
- `npm run check` — svelte-check.
- `npm run release` — установщик NSIS + portable zip в `release/`.
- `npm run icons` — перегенерировать иконки из `assets/*.svg`.

Rust в PATH может не быть: `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`.

## Устройство

- `src-tauri/src/app.rs` — контроллер: connect/disconnect, статус, цикл статистики, фоновое обновление подписок и геобаз.
- `src-tauri/src/xray/config_builder.rs` — генерация конфига. Порядок правил важен (защита от петли → DNS → LAN → блок IPv6 → реклама → пользовательские → РФ → финальное).
- `src-tauri/src/profile/` — парсер ссылок (vless/vmess/trojan/ss) и подписок (base64/plain, заголовки 3X-UI `subscription-userinfo`, `profile-update-interval`, `profile-title`).
- `src-tauri/src/routing.rs` — нормализация правил split-tunneling (пути приложений `C:/x/app.exe`, папки с `/` на конце, домены → `domain:` + punycode, IP/CIDR/`geoip:`).
- `src-tauri/src/commands.rs` — Tauri-команды; все async, чтобы не блокировать UI-поток.
- `src/` — UI. Ошибки бэкенда приходят кодами (`NEED_ADMIN`, `PORT_BUSY:10808`, …) и переводятся в `src/lib/i18n.ts`.

## Важные особенности

- Xray запрещает VLESS без TLS/REALITY до публичных адресов — это не баг конфига.
- Все свежие релизы Xray помечены pre-release, поэтому по умолчанию `corePrerelease = true`. В сборку кладётся версия из `scripts/fetch-core.ps1` (`$XrayVersion`), совпадающая с сервером Dogfather (3X-UI 3.8.5 / Xray 26.9.9).
- TUN требует прав администратора. Приложение стартует без UAC (`asInvoker`) и при необходимости перезапускается через `runas` с `--wait-pid` (иначе single-instance отклонит новую копию). Автозапуск в TUN — задача Планировщика с `/RL HIGHEST`.
- xray.exe привязан к Job Object с `KILL_ON_JOB_CLOSE`: не переживает падение клиента. Системный прокси, оставшийся после падения, снимается при следующем старте.
- Portable-режим включается файлом `portable` рядом с exe: данные в `.\data`, ядро в `.\xray`. Установленная версия: данные `%APPDATA%\LiteRay`; ядро используется прямо из `<папка установки>\resources\xray`, если она доступна на запись (NSIS ставит per-user в `%LOCALAPPDATA%\LiteRay`), иначе копируется в `%LOCALAPPDATA%\LiteRay-core` — обновлятору нужно право записи.
