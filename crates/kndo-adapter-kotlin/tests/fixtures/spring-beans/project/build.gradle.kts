plugins { kotlin("jvm") }
group = "demo"

// The pack's ACTIVATION gate: `kndo:spring` consults its rules only where the
// project declares a dependency on Spring. Without this line the annotations
// below are markers nothing interprets, which is the whole point.
dependencies {
    implementation("org.springframework.boot:spring-boot-starter-web:3.2.0")
}
