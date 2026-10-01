import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.compose.compiler)
    alias(libs.plugins.compose.multiplatform)
}

kotlin {
    jvmToolchain {
        languageVersion.set(JavaLanguageVersion.of(21))
    }
}

dependencies {
    implementation(project(":client"))
    implementation(compose.desktop.currentOs)
    implementation(libs.kotlinx.coroutines.swing)
}

// Development runs load the engine straight from Cargo's output directory.
val cargoProfile = providers.gradleProperty("orbit.cargoProfile").getOrElse("debug")
val hostLibrary = rootProject.layout.projectDirectory.file("target/$cargoProfile/${System.mapLibraryName("orbit_ffi")}")

compose.desktop {
    application {
        mainClass = "com.orbit.desktop.MainKt"
        jvmArgs += listOf("-Dorbit.native.library=${hostLibrary.asFile.absolutePath}")
        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb)
            packageName = "Orbit"
            packageVersion = "0.1.0"
        }
    }
}

tasks.matching { it.name == "run" || it.name == "runDistributable" }.configureEach {
    dependsOn(":shared:cargoBuildHost")
}
