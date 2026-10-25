package com.orbit.client.features.chat

import androidx.compose.ui.Modifier

/** Right-click on desktop. Phones use the bubble overflow button for the same menu. */
internal expect fun Modifier.onSecondaryPress(onPress: () -> Unit): Modifier
