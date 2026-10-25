@file:OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)

package com.orbit.client.features.chat

import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerButton
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.onPointerEvent

internal actual fun Modifier.onSecondaryPress(onPress: () -> Unit): Modifier =
    onPointerEvent(PointerEventType.Press) { event ->
        if (event.button == PointerButton.Secondary) onPress()
    }
