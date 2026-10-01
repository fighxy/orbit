import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose.compiler)
}

val ndkVersionPinned = "29.0.14206865"
val androidAbis = listOf("arm64-v8a", "x86_64")
val cargoProfile = providers.gradleProperty("orbit.cargoProfile").getOrElse("debug")
val generatedJniLibs = layout.buildDirectory.dir("generated/jniLibs")

android {
    namespace = "com.orbit.android"
    compileSdk = libs.versions.android.compileSdk.get().toInt()
    ndkVersion = ndkVersionPinned

    defaultConfig {
        applicationId = "com.orbit.messenger"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += androidAbis }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures { compose = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(generatedJniLibs.get().asFile.absolutePath)
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":client"))
    implementation(libs.androidx.activity.compose)
    implementation(libs.kotlinx.coroutines.android)
}

// Builds liborbit_ffi.so for each ABI with cargo-ndk (`cargo install cargo-ndk`).
val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    group = "orbit"
    description = "Builds orbit-ffi for Android ABIs ($cargoProfile)."
    val sdkDir = androidComponents.sdkComponents.sdkDirectory
    workingDir = rootDir
    doFirst {
        environment("ANDROID_NDK_HOME", sdkDir.get().dir("ndk/$ndkVersionPinned").asFile.absolutePath)
    }
    val args = mutableListOf("cargo", "ndk", "--platform", libs.versions.android.minSdk.get())
    androidAbis.forEach { args += listOf("-t", it) }
    args += listOf("-o", generatedJniLibs.get().asFile.absolutePath, "build", "-p", "orbit-ffi", "--locked")
    if (cargoProfile == "release") args += "--release"
    commandLine(args)
    inputs.files(fileTree(rootDir.resolve("crates")))
    inputs.files(rootDir.resolve("Cargo.toml"), rootDir.resolve("Cargo.lock"))
    outputs.dir(generatedJniLibs)
}

tasks.named("preBuild") { dependsOn(cargoNdkBuild) }
