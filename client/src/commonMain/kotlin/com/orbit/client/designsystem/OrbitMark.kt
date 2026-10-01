package com.orbit.client.designsystem

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Logo: a planet with an orbit ring and a satellite. */
@Composable
fun OrbitMark(size: Dp = 40.dp, modifier: Modifier = Modifier) {
    val planet = MaterialTheme.colorScheme.primary
    val ring = MaterialTheme.colorScheme.primary.copy(alpha = 0.45f)
    val satellite = MaterialTheme.colorScheme.tertiary
    Canvas(modifier.size(size)) {
        val w = this.size.width
        drawOval(
            color = ring,
            topLeft = Offset(w * 0.04f, w * 0.30f),
            size = Size(w * 0.92f, w * 0.40f),
            style = Stroke(width = w * 0.05f),
        )
        drawCircle(color = planet, radius = w * 0.24f, center = Offset(w / 2, w / 2))
        drawCircle(color = satellite, radius = w * 0.08f, center = Offset(w * 0.88f, w * 0.42f))
    }
}
