package demo

interface Provider

open class Base(val provider: Provider)

// Referenced ONLY as an argument to a superclass constructor call, one line below. Internal,
// so no library-surface root can keep it alive — the reference has to.
internal object MyProvider : Provider

internal class MyMeta : Base(MyProvider)

class Hasher(val cost: Int = DEFAULT_COST) {
    // Referenced ONLY as a default parameter value in this same class's constructor, and
    // private, so again nothing but that reference can keep it alive.
    private companion object {
        private const val DEFAULT_COST = 65536
    }
}

fun main() {
    println(MyMeta().provider)
    println(Hasher().cost)
}
