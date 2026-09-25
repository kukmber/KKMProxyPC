# KKMProxy для Windows

Настольная версия [KKMProxy](https://github.com/kukmber/KKMProxy): VPN на ядре mihomo,
локальный прокси для Telegram и обход блокировок без VPN.

Стек: **Tauri 2** (Rust) + React + **Fluent UI 2** — окно на Mica, светлая/тёмная тема
системы, трей, один экземпляр.

## Разработка

```
npm install
npm run tauri dev          # окно приложения с горячей перезагрузкой интерфейса
cd src-tauri && cargo test # разбор подписок и сборка конфига
```

Нужны Node.js, Rust (MSVC) и Visual Studio Build Tools с C++.
Данные приложения: `%LOCALAPPDATA%\io.github.kukmber.kkmproxypc`.

## Устройство

| Файл | Что делает |
|---|---|
| `src-tauri/src/vpn.rs` | запуск/остановка mihomo, проверка готовности, системный прокси |
| `src-tauri/src/config.rs` | итоговый конфиг: порты, контроллер, DNS, TUN, предзагрузка правил |
| `src-tauri/src/subscription.rs` | подписки: YAML, base64, ссылки vless/hysteria2/trojan/ss/vmess |
| `src-tauri/src/cores.rs` | ядра: версии, проверка и обновление с GitHub по кнопке |
| `src-tauri/src/tgproxy.rs` | прокси для Telegram: запуск, ссылка `tg://proxy`, подключения |
| `src-tauri/src/child.rs` | общий запуск ядер: Job Object, журнал, падения |
| `src-tauri/src/winsys.rs` | права администратора, Job Object, версия Windows |
| `src/pages`, `src/components` | интерфейс |

Решения из брифа, которые уже заложены:
- rule-provider'ы и geo-базы скачиваются заранее напрямую; не скачанные временно
  исключаются и догружаются через VPN с `PUT /configs?force=true`;
- готовность ядра — запрос к своему контроллеру через SOCKS-порт ядра
  (правило `IP-CIDR,127.0.0.1/32,DIRECT,no-resolve` первым);
- DNS: `respect-rules: true` и 77.88.8.8 в `proxy-server-nameserver`;
- задержка узла: 3 попытки, повтор неответивших раз в 30 секунд;
- порт контроллера и секрет — случайные при каждом запуске;
- ядро привязано к Job Object — умирает вместе с приложением; системный прокси
  восстанавливается и после аварийного завершения.

## План

1. ~~Скелет: окно, mihomo, подписки, системный прокси, состояние~~
2. ~~Ядра: версии, проверка раз в сутки, обновление кнопкой на каждое~~
3. ~~Прокси для Telegram — Rust-порт valnesfjord/tg-ws-proxy-rs (консольный, 4 МБ)~~
4. Обход блокировок — **zapret** (`winws.exe`, WinDivert, нужны права администратора)
   вместо ByeDPI из Android-версии: стратегии, списки доменов, автоподбор
5. TUN, список соединений, правила по программам (`PROCESS-NAME`)
6. Автозапуск, установщик NSIS с ядрами, автообновление приложения
