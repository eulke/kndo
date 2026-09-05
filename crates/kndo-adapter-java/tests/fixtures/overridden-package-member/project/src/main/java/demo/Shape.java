package demo;

public abstract class Shape {
  public double describe() {
    return area();
  }

  abstract double area();

  double onlyHere() {
    return 1.0;
  }

  private double used() {
    return onlyHere();
  }

  public double both() {
    return used();
  }
}
