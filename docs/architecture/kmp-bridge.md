# Мост Rust ↔ Kotlin Multiplatform (ABI v3)

Статус: реализовано на этапе 0. Источник истины — `crates/orbit-ffi/src` и
сгенерированный `crates/orbit-ffi/include/orbit.h`; этот документ объясняет
контракт и причины решений.

## Слои

| Слой | Где | Ответственность |
|---|---|---|
| `orbit-core` | `crates/orbit-core` | Идентичность, хранилище, движок, JSON-протокол команд и событий |
| `orbit-ffi` | `crates/orbit-ffi` | C ABI, JNI, реестр handle, перехват panic, коды ошибок |
| KMP SDK | `shared/` | `NativeLibrary`/`NativeEngine`, `OrbitSdk`, `OrbitClient`, secure stores |
| UI | `client/`, `apps/` | Compose-экраны и состояние; к ключам и БД доступа не имеет |

Один и тот же набор безопасных операций (`ops.rs`) вызывается из C ABI (iOS через
cinterop) и из JNI (Android, JVM desktop). Kotlin не дублирует протокол,
криптографию и хранение.

## Функции

| C ABI | JNI (`com.orbit.sdk.bridge.OrbitJni`) | Назначение |
|---|---|---|
| `orbit_abi_version` | `nativeAbiVersion` | Версия контракта; SDK отказывается работать при несовпадении |
| `orbit_identity_generate` | `nativeGenerateIdentity` | Новый секрет аккаунта и устройства для secure store |
| `orbit_identity_lock` / `_unlock` | `nativeLockIdentity` / `nativeUnlockIdentity` | Запечатать секрет код-паролем и открыть его |
| `orbit_engine_open` | `nativeOpen` | Открыть аккаунт: `{"data_dir": "/abs/path"}` + секрет |
| `orbit_engine_submit` | `nativeSubmit` | JSON-команда → request ID |
| `orbit_engine_wait_events` | `nativeWaitEvents` | Блокирующее ожидание пакета событий, до 60 с |
| `orbit_engine_cancel_wait` | `nativeCancelWait` | Разбудить ожидание пустым пакетом |
| `orbit_engine_close` | `nativeClose` | Закрыть движок; возвращается после остановки worker и снятия блокировки |
| `orbit_buffer_free` | — | Затереть и освободить буфер Rust |
| `orbit_last_error_message` | — | Текст последней ошибки потока |

JVM desktop дополнительно использует `com.orbit.sdk.platform.DesktopKeyring`
(`desktop_keyring.rs`): Keychain, Credential Manager, Secret Service. Это адаптер
secure store, не часть контракта движка.

## Правила ABI

- **Handle.** Движки адресуются числами `u64`, которые никогда не переиспользуются.
  Обращение к закрытому handle возвращает `closed`, а не обращается к освобождённой памяти.
- **Ошибки.** `0` — успех, иначе код `ORBIT_ERR_*` (1–18), одинаковый в C, JNI
  (`OrbitNativeException(code, message)`) и JSON (`snake_case`). Panic не пересекает границу.
- **Память.** У буфера один владелец; `orbit_buffer_free` затирает содержимое.
  Входные указатели могут быть `NULL` только при нулевой длине.
- **Секреты.** Секрет идентичности передаётся только между движком и secure store,
  Kotlin затирает копии сразу после использования. Сообщения об ошибках не содержат
  входных данных: ошибки разбора JSON не цитируют текст команды.
- **События.** Результат каждой команды приходит ровно одним событием
  `command_succeeded`/`command_failed` и никогда не отбрасывается. Уведомления
  (`message_added`, `profile_changed`, `contacts_changed`, `network_changed`) при переполнении очереди заменяются одним
  `resync_required`; клиент запрашивает snapshot заново.
- **Корреляция.** `OrbitClient` отправляет команду и регистрирует ожидание под одной
  блокировкой, поэтому результат не может прийти раньше регистрации.

## Протокол команд (JSON, тег `type`)

| Команда | Результат | Уведомление |
|---|---|---|
| `get_snapshot` | `snapshot { identity, profile, conversations, network }` | — |
| `list_messages { conversation_id, before_seq?, limit }` | `messages { page }` | — |
| `send_text { conversation_id, text }` | `message_saved { message }` | `message_added` |
| `register_node { node, registration_code? }` | `node_registered { network }` | `network_changed` |
| `create_invite` | `invite_created { text }` | — |
| `inspect_invite { text }` | `invite_inspected { preview }` | — |
| `accept_invite { text }` | `contact_added { contact }` | `contacts_changed` |
| `update_profile { display_name, about }` | `profile_updated { profile }` | `profile_changed` |

Rust отклоняет неизвестные поля команд; Kotlin игнорирует неизвестные поля
событий. Любое несовместимое изменение повышает `ORBIT_ABI_VERSION`.
Golden JSON-тесты (`shared/src/commonTest`) сверяют Kotlin-модели с выводом Rust.

## Код-пароль

Формат запечатанного секрета: `0x10 | m_cost u32 | t_cost u32 | p_cost u8 |
salt[16] | nonce[24] | ciphertext`. Ключ — Argon2id (64 МиБ, 3 прохода) от
код-пароля; шифр — XChaCha20-Poly1305, заголовок аутентифицирован. При открытии
параметры KDF ограничены, чтобы подделанный blob не требовал неограниченной памяти.

Код-пароль защищает ключ от процессов, которые могут читать системное хранилище
(на desktop это часто любые программы пользователя). Короткий числовой код лишь
замедляет перебор скопированного blob. Пауза после неудачных попыток в UI
хранится в памяти и не заменяет стоимость Argon2.

## Сборка

| Платформа | Библиотека | Как собирается |
|---|---|---|
| JVM desktop | `liborbit_ffi.so/.dylib/.dll` | `:shared:cargoBuildHost`; путь передаётся через `-Dorbit.native.library` |
| Android | `liborbit_ffi.so` для arm64-v8a, x86_64 | `:apps:android:cargoNdkBuild` (cargo-ndk, NDK 29) |
| iOS | `liborbit_ffi.a` | `cargoBuildIos*` на macOS; cinterop встраивает библиотеку в klib |

## Что не проверено

- Запуск iOS-моста и Keychain на устройстве или симуляторе (есть только компиляция в CI).
- Запуск Android-приложения на устройстве (APK собирается).
- Поведение Keystore/Keychain при блокировке устройства и восстановлении из backup.

## Доставка личных сообщений (ABI v3)

Отдельный сетевой actor держит исходящее QUIC-соединение с mailbox. Получение
использует `Wait`, а не периодические `Fetch`. Сетевой поток передаёт пачку
конвертов worker движка и ждёт его решения: `Ack` вызывается только после
успешного локального commit. Повреждённые/неавторизованные конверты записываются
как отклонённые до ACK, не блокируя остальные сообщения. Сбой storage ACK не даёт.

`message_added` означает upsert по MessageId: событие также меняет состояние
существующего сообщения. `queued` — локальная транзакция истории + outbox;
`mailbox` — узел подтвердил запись; `delivered` — проверенная квитанция адресата;
`received` — сохранённое входящее сообщение. Это не подтверждение прочтения.
Контакты, routing capabilities и конфигурация узла зашифрованы на диске.

Перед отправкой текста контакт завершает взаимный обмен подписанными карточками.
Текст можно поставить в очередь сразу после принятия приглашения; он остаётся
в outbox до подтверждения обмена. При retry и restart исходный ciphertext не
меняется. Close отменяет сетевой actor и активный long-poll.
