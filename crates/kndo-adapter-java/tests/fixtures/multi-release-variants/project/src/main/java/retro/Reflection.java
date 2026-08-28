package retro;

public class Reflection {
  public static Object call(Object proxy) {
    return DefaultMethodSupport.invoke(proxy);
  }
}
