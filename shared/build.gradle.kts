import org.jetbrains.kotlin.gradle.ExperimentalKotlinGradlePluginApi
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.plugin.KotlinHierarchyTemplate
import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType
import org.jetbrains.kotlin.konan.target.HostManager

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.android.kmp.library)
}

// The Rust engine is built by Cargo from the repository root. Kotlin code
// never reimplements protocol, crypto or storage logic.
val cargoProfile = providers.gradleProperty("orbit.cargoProfile").getOrElse("debug")
val cargoProfileArgs = if (cargoProfile == "release") listOf("--release") else emptyList()
val cargoTargetDir = rootProject.layout.projectDirectory.dir("target")
val ffiHeaderDir = rootProject.layout.projectDirectory.dir("crates/orbit-ffi/include")

val hostLibraryName = System.mapLibraryName("orbit_ffi")
val hostLibrary = cargoTargetDir.file("$cargoProfile/$hostLibraryName")

val cargoBuildHost = tasks.register<Exec>("cargoBuildHost") {
    group = "orbit"
    description = "Builds the orbit-ffi shared library for the host JVM ($cargoProfile)."
    workingDir = rootDir
    commandLine(listOf("cargo", "build", "-p", "orbit-ffi", "--locked") + cargoProfileArgs)
    inputs.files(fileTree(rootDir.resolve("crates")) { exclude("**/target/**") })
    inputs.files(rootDir.resolve("Cargo.toml"), rootDir.resolve("Cargo.lock"))
    outputs.file(hostLibrary)
}

kotlin {
    @OptIn(ExperimentalKotlinGradlePluginApi::class)
    applyHierarchyTemplate(KotlinHierarchyTemplate.default) {
        group("common") {
            // Android and JVM desktop share the JNI binding.
            group("jni") {
                // The Android KMP library plugin's target is not matched by
                // withAndroidTarget(), so select it by platform type.
                withCompilations { it.target.platformType == KotlinPlatformType.androidJvm }
                withJvm()
            }
        }
    }

    android {
        namespace = "com.orbit.sdk"
        compileSdk = libs.versions.android.compileSdk.get().toInt()
        minSdk = libs.versions.android.minSdk.get().toInt()
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }

    jvm {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }

    // Rust target triple for each Kotlin/Native target.
    val appleTargets = mapOf(
        iosArm64() to "aarch64-apple-ios",
        iosSimulatorArm64() to "aarch64-apple-ios-sim",
    )
    appleTargets.forEach { (target, rustTarget) ->
        val taskSuffix = target.name.replaceFirstChar { it.uppercase() }
        val staticLibDir = cargoTargetDir.dir("$rustTarget/$cargoProfile")
        val cargoBuild = tasks.register<Exec>("cargoBuild$taskSuffix") {
            group = "orbit"
            description = "Builds the orbit-ffi static library for $rustTarget ($cargoProfile)."
            onlyIf { HostManager.hostIsMac }
            workingDir = rootDir
            commandLine(listOf("cargo", "build", "-p", "orbit-ffi", "--locked", "--target", rustTarget) + cargoProfileArgs)
            outputs.file(staticLibDir.file("liborbit_ffi.a"))
        }
        target.compilations.getByName("main").cinterops.create("orbit") {
            definitionFile.set(project.file("src/nativeInterop/cinterop/orbit.def"))
            includeDirs(ffiHeaderDir)
            extraOpts("-libraryPath", staticLibDir.asFile.absolutePath)
        }
        tasks.named("cinteropOrbit$taskSuffix") { dependsOn(cargoBuild) }
        // The iOS framework is produced by :client, which exports this module.
    }

    sourceSets {
        commonMain.dependencies {
            api(libs.kotlinx.coroutines.core)
            implementation(libs.kotlinx.serialization.json)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation(libs.kotlinx.coroutines.test)
        }
    }
}

tasks.named<Test>("jvmTest") {
    dependsOn(cargoBuildHost)
    systemProperty("orbit.native.library", hostLibrary.asFile.absolutePath)
}
