plugins { id("java-library") }
dependencies {
    // implementation("com.commented:out:1.0")
    implementation("org.slf4j:slf4j-api:2.0.9")
    testImplementation(libs.junit.core)
}
