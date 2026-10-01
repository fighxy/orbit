# ADR 0001: ядро Rust, мосты и клиенты KMP

- Дата: 2026-10-01.
- Статус: направление принято; конкретные библиотеки и эксплуатационные пределы требуют экспериментов.
- Working tree (`feat/vertical-chat`), not a change to this decision: direct sessions use Iroh (`presets::N0`, ALPN `orbit/direct/1`). The invite carries an endpoint id, not a pasted socket. Mailbox QUIC (`orbit/mailbox/1`) still dials `hex@ip:port` with relays disabled. The gates in the table below (device matrix, network changes, battery, OpenMLS, an SFU) are not closed. The pairwise group in the tree is not MLS. Voice notes are chunked 16 kHz PCM WAV in direct chats and saved messages. Android and desktop record and play them. iOS source records and plays through AVFoundation and scales a photo to a JPEG of at most 32 KiB. That source was not compiled on Windows and was not run on a device. Video notes and voice rooms are not started. A 24-word phrase restores the same Rust identity and is not on the screen or in the FFI. ABI is 6. Schema version is 7.
- Основание: план и архитектурное ревью Orbit.

## Контекст

Первоначальные README, карта Holepunch и KMP-заготовки описывали разные способы
построения клиента: JS bridge и реализацию протокола на Kotlin. Для дальнейшей
работы требуется одна согласованная граница ответственности.

Orbit должен поддерживать собственный протокол, сообщества, постоянные голосовые
комнаты, broadcast channels, личные/групповые чаты и сохранённые медиафайлы.

## Решение

1. Rust владеет протоколом, доменными правилами, состоянием криптографии,
   транзакционным хранением сообщений, репликацией и доставкой.
2. KMP владеет публичным SDK, мостами, ViewModel, клиентским UI и адаптерами ОС.
   Для общего UI предлагается Compose Multiplatform.
3. Начало Rust workspace — `orbit-core` и `orbit-ffi`. Остальные подсистемы
   сначала являются внутренними модулями. Будущие crates извлекаются по
   проверенным границам.
4. iOS использует Kotlin/Native cinterop к C ABI; Android и JVM desktop — JNI.
   Память, отмена, завершение engine и event subscription входят в ABI-контракт.
5. Orbit определяет собственный версионированный application protocol.
   Совместимость с wire/storage форматами Holepunch не требуется.
6. Holepunch служит картой функций и архитектурных идей. Собственные JS runtime,
   QUIC, криптопримитивы и все вспомогательные библиотеки не являются целью.
7. Сообщения, outbox и crypto state имеют согласованную durable запись.
   Сетевой ciphertext не считается достаточным backup локальной истории.
8. Persistent data sync и live media используют разные транспортные требования.
   SFU, relay, mailbox и push различаются по функциям; self-hosting предусмотрен.

## Следствия

- `shared/` развивается в KMP SDK, а не во вторую реализацию Rust-домена.
- Rust — единственный владелец message storage. Room/SwiftData не становятся
  самостоятельными источниками тех же сообщений.
- Минимальный UI создаётся вместе с ядром. Голос и iOS lifecycle проверяются рано.
- JVM-only `desktopMain` нельзя наследовать из Kotlin/Native desktop targets.
- Публичные identity DTO не содержат seed/секреты.
- `policy_version` и криптографическая epoch не отождествляются.
- Сохранённые голосовые сообщения и кружки используют blob delivery, живой голос — media pipeline.

## Открытые решения

| Решение | Кандидат / направление | Условие выбора |
|---|---|---|
| Data transport | Iroh | Реальные устройства, direct/relay, смена сети, энергопотребление |
| E2EE | OpenMLS | Persistence, Commit/Welcome, авторизация, offline/rejoin/recovery |
| Media stack | Готовый self-hosted SFU для baseline; LiveKit как кандидат | KMP adapters, iOS/Android/desktop media support, lifecycle и E2EE hooks |
| Собственный SFU | Rust + str0m | Сопоставимые испытания относительно baseline и обоснование разработки |
| Управление комнатой | Один делегированный controller для MVP | Конкретные правила выдачи/отзыва прав, согласования и потери связи |
| Пределы продукта | Число участников, размеры медиа, retention | Измерения, затраты и UX; значения пока не объявлены |

LiveKit использует Go. Он может быть сторонним baseline, но не считается
реализацией собственного Rust-SFU. Окончательный выбор медиасервера открыт.

## Проверка решения

Первое доказательство жизнеспособности — устанавливаемый KMP клиент, который
вызывает Rust, передаёт приватное сообщение и восстанавливается после restart.
Ранний отдельный эксперимент — голосовая комната на трёх устройствах. Полная
матрица задач и критериев находится в [roadmap](../roadmap.md).

См. [архитектуру](../architecture/rust-kmp.md) и
[ревью](../reviews/2026-10-01-architecture-review.md).
