package demo.core

class ApiTest {
    fun checksTheProbe() {
        check(probe() == 7)
        check(greet() == "HELLO")
    }
}
