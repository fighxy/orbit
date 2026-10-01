package com.orbit.sdk

import kotlinx.coroutines.CoroutineDispatcher

/** Dispatcher for blocking native calls. */
internal expect val defaultIoDispatcher: CoroutineDispatcher
