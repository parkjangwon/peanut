package com.peanut.sdk;

public final class PeanutException extends RuntimeException {
    private final int statusCode;
    private final String responseBody;

    public PeanutException(int statusCode, String responseBody) {
        super("Peanut request failed with HTTP " + statusCode + ": " + responseBody);
        this.statusCode = statusCode;
        this.responseBody = responseBody;
    }

    public PeanutException(String message, Throwable cause) {
        super(message, cause);
        this.statusCode = -1;
        this.responseBody = null;
    }

    public int statusCode() {
        return statusCode;
    }

    public String responseBody() {
        return responseBody;
    }
}
