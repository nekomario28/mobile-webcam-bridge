plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val appVersion = rootProject.file("../VERSION").readText().trim()
val appVersionParts = Regex("^(\\d+)\\.(\\d+)\\.(\\d+)$")
    .matchEntire(appVersion)?.groupValues?.drop(1)?.map(String::toInt)
    ?: error("VERSION must use major.minor.patch")
val releaseStoreFile = providers.environmentVariable("AMB_RELEASE_STORE_FILE").orNull
val releaseStorePassword = providers.environmentVariable("AMB_RELEASE_STORE_PASSWORD").orNull
val releaseKeyAlias = providers.environmentVariable("AMB_RELEASE_KEY_ALIAS").orNull
val releaseKeyPassword = providers.environmentVariable("AMB_RELEASE_KEY_PASSWORD").orNull
val releaseSigningAvailable = listOf(
    releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword
).all { !it.isNullOrBlank() }
val partialReleaseSigning = listOf(
    releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword
).any { !it.isNullOrBlank() } && !releaseSigningAvailable
check(!partialReleaseSigning) { "Set all AMB_RELEASE_* signing variables or none of them" }

android {
    namespace = "dev.nekomario.amb"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.nekomario.amb"
        minSdk = 26
        targetSdk = 35
        versionCode = appVersionParts[0] * 1_000_000 +
            appVersionParts[1] * 1_000 + appVersionParts[2]
        versionName = appVersion
    }

    signingConfigs {
        if (releaseSigningAvailable) {
            create("release") {
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        getByName("release") {
            if (releaseSigningAvailable) signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    jvmToolchain(17)
}
