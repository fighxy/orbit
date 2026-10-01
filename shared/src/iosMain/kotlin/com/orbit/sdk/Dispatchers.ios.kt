package com.orbit.sdk

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO

internal actual val defaultIoDispatcher: CoroutineDispatcher = Dispatchers.IO
