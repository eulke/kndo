plugins { id("java-library") }
sourceSets {
    main { java { srcDirs("src/generated/java", "src/main/java") } }
}
dependencies {
    api(project(":core"))
    implementation(libs.guava)
    testImplementation(libs.junit.core)
}
