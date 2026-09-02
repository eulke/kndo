package demo;

public final class Classify {
    private Classify() {}

    public static String grade(int score) {
        if (score >= 90) {
            return "A";
        } else if (score >= 75) {
            return "B";
        } else if (score >= 60) {
            return "C";
        }
        return "F";
    }

    public static boolean dark(int lux) {
        return lux < 10;
    }

    public static int neverRan(int x) {
        int y = x * 2;
        return y + 1;
    }
}
