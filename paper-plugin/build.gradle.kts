plugins {
    java
}

group = "de.ayynoxs"
version = "0.4.0"

repositories {
    mavenCentral()
    maven("https://repo.papermc.io/repository/maven-public/")
}

dependencies {
    compileOnly("io.papermc.paper:paper-api:26.2.build.112-stable")
}

java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(25))
    }
}

tasks.jar {
    archiveFileName.set("AyynoxsMapSync-${project.version}.jar")
}
