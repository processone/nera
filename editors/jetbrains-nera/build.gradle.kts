plugins {
    id("org.jetbrains.intellij.platform") version "2.13.1"
    kotlin("jvm") version "2.1.0"
}

group = "net.processone"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        if (System.getenv("CI") != null) {
            intellijIdeaUltimate("2025.2")
        } else {
            local("/Applications/RustRover.app/Contents")
        }
        // Native lexer replaces TextMate — no bundled plugin dependency needed
    }
    testImplementation("junit:junit:4.13.2")
}

kotlin {
    jvmToolchain(21)
}

intellijPlatform {
    pluginConfiguration {
        id = "net.processone.nera"
        name = "Nera Erlang"
        version = project.version.toString()
        description = "Language support for Erlang projects using the nera compiler"
        vendor {
            name = "ProcessOne"
            url = "https://www.process-one.net"
        }
        ideaVersion {
            sinceBuild = "253"
        }
    }
}

tasks {
    buildSearchableOptions {
        enabled = false
    }
}
