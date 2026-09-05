package demo;

public class Square extends Shape {
  private final double side;

  public Square(double side) {
    this.side = side;
  }

  double area() {
    return side * side;
  }
}
