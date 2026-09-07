package com.foo;

import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.io.Serializable;

/** A base the project does not contain, whose requirements no call site names. */
public final class Ticket implements Comparable<Ticket>, Serializable {
  private final int seat;

  public Ticket(int seat) {
    this.seat = seat;
  }

  // Comparable's one requirement. No `@Override` — Java does not ask for one,
  // and the base is in the JDK, so the graph can never resolve it.
  public int compareTo(Ticket other) {
    return Integer.compare(seat, other.seat);
  }

  // The serialization runtime calls these reflectively. Private, so no
  // supertype declares them and no source line anywhere spells their names.
  private void writeObject(ObjectOutputStream out) throws java.io.IOException {
    out.defaultWriteObject();
  }

  private void readObject(ObjectInputStream in) throws java.io.IOException, ClassNotFoundException {
    in.defaultReadObject();
  }

  // Same shape, same file, and nothing requires it.
  private void auditTrail() {}
}
