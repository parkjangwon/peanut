package com.peanut.sdk;

import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

final class Json {
    private Json() {
    }

    static Optional<String> extractString(String json, String field) {
        Pattern pattern = Pattern.compile("\"" + Pattern.quote(field) + "\"\\s*:\\s*\"((?:\\\\.|[^\"])*)\"");
        Matcher matcher = pattern.matcher(json);
        if (!matcher.find()) {
            return Optional.empty();
        }
        return Optional.of(unescape(matcher.group(1)));
    }

    private static String unescape(String value) {
        return value
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\/", "/")
                .replace("\\b", "\b")
                .replace("\\f", "\f")
                .replace("\\n", "\n")
                .replace("\\r", "\r")
                .replace("\\t", "\t");
    }
}
