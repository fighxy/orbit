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

// The PNG icon is also loaded at runtime for the window and taskbar.
sourceSets.main {
    resources.srcDir("icons")
}

dependencies {
    implementation(project(":client"))
    implementation(compose.desktop.currentOs)
    implementation(libs.kotlinx.coroutines.swing)
}

// The Rust engine is shipped inside the application as a Compose app
// resource for the build host's OS and architecture. Release packages are
// built with -Porbit.cargoProfile=release.
val cargoProfile = providers.gradleProperty("orbit.cargoProfile").getOrElse("debug")
val nativeLibraryName = System.mapLibraryName("orbit_ffi")
val hostLibrary = rootProject.layout.projectDirectory.file("target/$cargoProfile/$nativeLibraryName")
val nativeResources = layout.buildDirectory.dir("native-resources")

/** Compose resource folder name for this host, e.g. `windows-x64`. */
val hostResourceDir: String = run {
    val os = System.getProperty("os.name").lowercase()
    val arch = when (val a = System.getProperty("os.arch").lowercase()) {
        "amd64", "x86_64" -> "x64"
        "aarch64", "arm64" -> "arm64"
        else -> error("unsupported architecture $a")
    }
    when {
        os.contains("win") -> "windows-$arch"
        os.contains("mac") -> "macos-$arch"
        else -> "linux-$arch"
    }
}

val copyNativeLibrary = tasks.register<Copy>("copyNativeLibrary") {
    group = "orbit"
    description = "Places the orbit-ffi library into the application resources."
    dependsOn(":shared:cargoBuildHost")
    from(hostLibrary)
    into(nativeResources.map { it.dir(hostResourceDir) })
}

compose.desktop {
    application {
        mainClass = "com.orbit.desktop.MainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Msi, TargetFormat.Exe, TargetFormat.Dmg, TargetFormat.Deb)
            packageName = "Orbit"
            packageVersion = "0.1.0"
            description = "Orbit messenger"
            vendor = "Orbit"
            appResourcesRootDir.set(nativeResources)
            // Modules reported by :apps:desktop:suggestRuntimeModules.
            modules("java.instrument", "jdk.unsupported")
            windows {
                // Installs into the user's profile; no administrator rights needed.
                perUserInstall = true
                menu = true
                menuGroup = "Orbit"
                shortcut = true
                dirChooser = false
                // Must stay constant so new versions upgrade the installed one.
                upgradeUuid = "6f1c2a7e-4b8d-4e5a-9c3f-0d7b8e2a1f64"
                iconFile.set(project.file("icons/orbit.ico"))
            }
            linux {
                iconFile.set(project.file("icons/orbit.png"))
            }
        }
    }
}

tasks.matching { it.name == "prepareAppResources" }.configureEach {
    dependsOn(copyNativeLibrary)
}
