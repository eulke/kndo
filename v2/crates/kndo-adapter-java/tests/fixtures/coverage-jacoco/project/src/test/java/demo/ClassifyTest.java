package demo;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

class ClassifyTest {
    @Test
    void gradesTheTop() {
        assertEquals("A", Classify.grade(95));
        assertEquals("B", Classify.grade(80));
    }

    @Test
    void darkness() {
        assertTrue(Classify.dark(3));
    }
}
