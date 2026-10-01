package com.orbit.client.designsystem

import com.orbit.sdk.OrbitException
import com.orbit.sdk.bridge.OrbitErrorCode

/** UI text. Kept in one place so localization can replace it later. */
object Strings {
    const val appName = "Orbit"
    const val loading = "Открываем хранилище…"

    const val onboardingTitle = "Добро пожаловать в Orbit"
    const val onboardingBody =
        "Аккаунт создаётся на этом устройстве: без телефона и почты. " +
            "Ключ аккаунта хранится в защищённом хранилище системы, " +
            "а сообщения на диске зашифрованы ключом устройства."
    const val onboardingLimits =
        "Сейчас доступны локальные заметки. Переписка между устройствами, " +
            "резервная копия ключа и восстановление появятся на следующих этапах."
    const val createAccount = "Создать аккаунт"
    const val creatingAccount = "Создаём ключи…"

    const val secureStorageTitle = "Системное хранилище ключей недоступно"
    const val secureStorageBody =
        "Orbit хранит ключ аккаунта только в защищённом хранилище операционной системы " +
            "и не сохраняет его в открытом виде. Пока хранилище недоступно, аккаунт открыть нельзя."
    const val secureStorageLinuxHint =
        "Linux: запустите службу Secret Service (GNOME Keyring или KWallet) и разблокируйте связку ключей."
    const val openFailedTitle = "Не удалось открыть аккаунт"
    const val retry = "Повторить"
    const val details = "Подробности"

    const val chats = "Чаты"
    const val savedMessages = "Избранное"
    const val savedMessagesSubtitle = "Хранится только на этом устройстве"
    const val noMessagesYet = "Пока пусто"
    const val emptyChatTitle = "Ваши заметки"
    const val emptyChatBody =
        "Сообщения здесь сохраняются на устройстве в зашифрованном виде и пока никуда не отправляются."
    const val selectChat = "Выберите чат"
    const val composerPlaceholder = "Заметка…"
    const val send = "Отправить"
    const val back = "Назад"
    const val savedLocally = "Сохранено на этом устройстве"
    const val loadingOlder = "Загружаем более ранние сообщения…"
    const val accountKey = "Ключ аккаунта"
    const val dismiss = "Закрыть"

    fun describe(error: OrbitException): String = when (error.code) {
        OrbitErrorCode.StorageLocked ->
            "Этот аккаунт уже открыт другим окном или процессом Orbit. Закройте его и повторите."
        OrbitErrorCode.IdentityMismatch, OrbitErrorCode.StorageKeyMismatch ->
            "Локальные данные принадлежат другому ключу устройства. Данные не изменены."
        OrbitErrorCode.Corrupted -> "Локальные данные повреждены: ${error.message}"
        OrbitErrorCode.UnsupportedStorageVersion -> "Данные созданы более новой версией Orbit. Обновите приложение."
        OrbitErrorCode.InvalidIdentity -> "Сохранённый ключ аккаунта повреждён или имеет неизвестный формат."
        OrbitErrorCode.Closed -> "Хранилище закрыто."
        OrbitErrorCode.Busy -> "Слишком много операций одновременно. Повторите через мгновение."
        OrbitErrorCode.InvalidArgument -> "Некорректный запрос: ${error.message}"
        else -> error.message ?: error.code.wireName
    }
}
