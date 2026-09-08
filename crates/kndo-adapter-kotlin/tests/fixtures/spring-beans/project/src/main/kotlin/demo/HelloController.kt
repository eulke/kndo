package demo

import org.springframework.web.bind.annotation.GetMapping
import org.springframework.web.bind.annotation.RestController

// Nothing in the project constructs this: the container finds it by component
// scan and calls `hello` when a request arrives.
@RestController
internal class HelloController {
    @GetMapping("/hello")
    fun hello(): String = "hello"
}
