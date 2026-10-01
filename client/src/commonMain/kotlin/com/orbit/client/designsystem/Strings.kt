package com.orbit.client.designsystem

import com.orbit.sdk.OrbitException
import com.orbit.sdk.bridge.OrbitErrorCode

/** UI text. Kept in one place so localization can replace it later. */
object Strings {
    const val localNetworkHint = "Для узла в вашей Wi-Fi-сети Android запросит доступ к устройствам поблизости."
    const val localNetworkDenied = "Доступ к локальной сети не разрешён. Для узла в Wi-Fi включите разрешение «Устройства поблизости» в настройках Orbit. Узел с публичным адресом остаётся доступен."
    const val appName = "Orbit"
    const val loading = "Открываем хранилище…"

    const val onboardingTitle = "Добро пожаловать в Orbit"
    const val onboardingBody =
        "Аккаунт создаётся на этом устройстве: без телефона и почты. " +
            "Ключ аккаунта хранится в защищённом хранилище системы, " +
            "а сообщения на диске зашифрованы ключом устройства."
    const val onboardingLimits = "После создания аккаунта откройте «Контакты», подключитесь к узлу и обменяйтесь приглашениями. Резервное копирование и восстановление пока недоступны."
    const val start = "Начать"
    const val next = "Далее"
    const val createAccount = "Создать аккаунт"
    const val creatingAccount = "Создаём ключи…"

    const val profileStepTitle = "Как вас называть?"
    const val profileStepBody = "Это имя увидят ваши собеседники в приглашении и списке чатов."
    const val displayName = "Имя"
    const val about = "О себе"
    const val aboutHint = "Необязательно"
    const val nameRequired = "Укажите имя"
    const val nameTooLong = "Имя длиннее 64 символов"
    const val aboutTooLong = "Текст «О себе» длиннее 140 символов"

    const val passcodeStepTitle = "Код-пароль"
    const val passcodeStepBody =
        "Код-пароль шифрует ключ аккаунта: без него Orbit не откроется на этом устройстве, " +
            "даже если кто-то получит доступ к системному хранилищу ключей."
    const val passcodeWarning =
        "Если забыть код-пароль, восстановить доступ к аккаунту пока нельзя. " +
            "Длинная фраза надёжнее короткого числового кода."
    const val passcode = "Код-пароль"
    const val currentPasscode = "Текущий код-пароль"
    const val newPasscode = "Новый код-пароль"
    const val confirmPasscode = "Повторите код-пароль"
    const val passcodeTooShort = "Не короче 4 символов"
    const val passcodeMismatch = "Код-пароли не совпадают"
    const val setPasscodeAndCreate = "Установить и создать"
    const val skip = "Пропустить"

    const val lockTitle = "Orbit заблокирован"
    const val lockBody = "Введите код-пароль, чтобы открыть аккаунт."
    const val unlock = "Разблокировать"
    const val unlocking = "Проверяем…"
    const val wrongPasscode = "Неверный код-пароль"
    const val forgotPasscode =
        "Без код-пароля аккаунт на этом устройстве не открыть. Восстановление из резервной копии появится позже."
    fun cooldown(seconds: Int) = "Слишком много попыток. Повторите через $seconds с"
    fun failedAttempts(count: Int) = "Неудачных попыток: $count"

    const val secureStorageTitle = "Системное хранилище ключей недоступно"
    const val secureStorageBody =
        "Orbit хранит ключ аккаунта только в защищённом хранилище операционной системы " +
            "и не сохраняет его в открытом виде. Пока хранилище недоступно, аккаунт открыть нельзя."
    const val secureStorageLinuxHint =
        "Linux: запустите службу Secret Service (GNOME Keyring или KWallet) и разблокируйте связку ключей."
    const val openFailedTitle = "Не удалось открыть аккаунт"
    const val retry = "Повторить"
    const val details = "Подробности"

    const val contacts = "Контакты и подключение"
    const val contact = "Контакт"
    const val deliveryServer = "Подключение к узлу"
    const val deliveryServerHint = "Введите адрес узла, который вы получили от его владельца. Оба собеседника могут пользоваться одним узлом."
    const val nodeAddress = "Адрес узла"
    const val registrationCode = "Код регистрации (если нужен)"
    const val registerOnServer = "Зарегистрироваться / подключиться"
    const val serverNotConfigured = "Узел ещё не выбран"
    const val serverConnecting = "Подключаемся…"
    const val serverConnected = "Подключено"
    const val serverOffline = "Нет подключения. Повторяем автоматически"
    const val myInvitation = "Моё приглашение"
    const val invitationHint = "Передайте приглашение собеседнику. После принятия вы появитесь в контактах друг у друга. Ссылка действует 7 дней."
    const val createInvitation = "Создать приглашение"
    const val copyInvitationHint = "Выделите и скопируйте ссылку, затем передайте её собеседнику."
    const val addContact = "Добавить контакт"
    const val pasteInvitation = "Вставьте orbit://invite/…"
    const val verifyInvitation = "Проверить приглашение"
    const val verifyContactKey = "Сверьте этот ключ с собеседником по знакомому вам каналу связи."
    const val messageQueued = "В очереди"
    const val messageOnServer = "На узле"
    const val messageDelivered = "Доставлено"
    const val contactPending = "Завершаем обмен контактами…"
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

    const val settings = "Настройки"
    const val today = "Сегодня"
    const val yesterday = "Вчера"
    const val scrollToLatest = "К последним сообщениям"
    const val appearanceSection = "Оформление и ввод"
    const val theme = "Тема"
    const val themeSystem = "Как в системе"
    const val themeLight = "Светлая"
    const val themeDark = "Тёмная"
    const val sendWith = "Отправка сообщения"
    const val sendWithEnter = "Enter — отправить, Shift+Enter — новая строка"
    const val sendWithCtrlEnter = "Ctrl+Enter — отправить, Enter — новая строка"
    const val shortcutsHint = "Ctrl+, — настройки, Esc — назад"
    const val messageTooLong = "Сообщение слишком длинное"
    const val profileSection = "Профиль"
    const val save = "Сохранить"
    const val saved = "Сохранено"
    const val accountSection = "Аккаунт"
    const val accountKeyFull = "Публичный ключ аккаунта"
    const val deviceKey = "Ключ этого устройства"
    const val accountKeyExplanation =
        "Ключи созданы на этом устройстве. Секретная часть хранится в системном хранилище ключей " +
            "и не покидает Orbit."
    const val securitySection = "Безопасность"
    const val passcodeOn = "Включён: ключ аккаунта зашифрован код-паролем"
    const val passcodeOff = "Выключен: ключ защищён только системным хранилищем"
    const val setPasscode = "Установить"
    const val changePasscode = "Изменить"
    const val disablePasscode = "Отключить"
    const val lockNow = "Заблокировать сейчас"
    const val cancel = "Отмена"
    const val confirm = "Подтвердить"
    const val aboutSection = "О приложении"
    const val version = "Версия"
    const val storageNote = "Сообщения хранятся на этом устройстве и зашифрованы ключом устройства."
    const val networkNote = "Личная переписка идёт через выбранный вами узел. В фоне на телефоне доставка может приостанавливаться до открытия приложения."
    const val notReady = "Аккаунт не открыт"

    fun describe(error: OrbitException): String = when (error.code) {
        OrbitErrorCode.StorageLocked ->
            "Этот аккаунт уже открыт другим окном или процессом Orbit. Закройте его и повторите."
        OrbitErrorCode.IdentityMismatch, OrbitErrorCode.StorageKeyMismatch ->
            "Локальные данные принадлежат другому ключу устройства. Данные не изменены."
        OrbitErrorCode.Corrupted -> "Локальные данные повреждены: ${error.message}"
        OrbitErrorCode.UnsupportedStorageVersion -> "Данные созданы более новой версией Orbit. Обновите приложение."
        OrbitErrorCode.InvalidIdentity -> "Сохранённый ключ аккаунта повреждён или имеет неизвестный формат."
        OrbitErrorCode.WrongPasscode -> wrongPasscode
        OrbitErrorCode.NetworkNotConfigured -> "Сначала зарегистрируйтесь на узле в разделе «Контакты»."
        OrbitErrorCode.InvalidInvite -> "Приглашение недействительно, просрочено или ключ контакта изменился."
        OrbitErrorCode.Network -> "Не удалось подключиться к узлу. Проверьте адрес, код регистрации и доступность сервера."
        OrbitErrorCode.Closed -> "Хранилище закрыто."
        OrbitErrorCode.Busy -> "Слишком много операций одновременно. Повторите через мгновение."
        OrbitErrorCode.InvalidArgument -> "Некорректный запрос: ${error.message}"
        else -> error.message ?: error.code.wireName
    }
}
