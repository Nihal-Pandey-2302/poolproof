// poolproof's pgjdbc program: runs a case's steps and prints one JSON line per step.
//
// The protocol is in drivers/README.md. In short: steps come on stdin, one per line
// (client, SQL, comma-separated integer parameters, tab-separated). Each client gets one
// connection, opened at the start and never reopened: no pool. Every step is reported,
// so a driver that recovers on the next call still shows the step that failed.

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.net.SocketTimeoutException;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;

import org.postgresql.util.DriverInfo;
import org.postgresql.util.PSQLException;

public final class Smoke {
    record Step(String client, String sql, int[] params) {}

    /** A client is a connection, or the error line every one of its steps reports. */
    record Client(Connection conn, String error) {}

    public static void main(String[] args) throws Exception {
        String variant = System.getenv("PP_VARIANT");
        int ms = Integer.parseInt(System.getenv("PP_STEP_TIMEOUT_MS"));
        // pgjdbc's timeouts are whole seconds.
        String seconds = Integer.toString((ms + 999) / 1000);
        List<Step> steps = readSteps();
        emit("{\"driver\":\"pgjdbc\",\"version\":" + json(DriverInfo.DRIVER_VERSION) + "}");

        Properties props = new Properties();
        props.setProperty("user", System.getenv("PP_USER"));
        props.setProperty("password", System.getenv("PP_PASSWORD"));
        props.setProperty("sslmode", System.getenv("PP_SSLMODE"));
        props.setProperty("connectTimeout", seconds);
        props.setProperty("loginTimeout", seconds);
        props.setProperty("socketTimeout", seconds);
        // The default, said out loud: no savepoints, so no retry after an error.
        props.setProperty("autosave", "never");
        switch (variant) {
            case "default" -> { }
            case "no-prepare" -> props.setProperty("prepareThreshold", "0");
            default -> throw new IllegalArgumentException("unknown variant " + variant);
        }
        String host = System.getenv("PP_HOST");
        if (host.contains(":")) {
            host = "[" + host + "]";
        }
        String url = "jdbc:postgresql://" + host + ":" + System.getenv("PP_PORT") + "/"
                + URLEncoder.encode(System.getenv("PP_DBNAME"), StandardCharsets.UTF_8).replace("+", "%20");

        Map<String, Client> clients = new LinkedHashMap<>();
        int attempts = 0;
        for (Step s : steps) {
            if (clients.containsKey(s.client())) {
                continue;
            }
            attempts++;
            try {
                clients.put(s.client(), new Client(DriverManager.getConnection(url, props), null));
            } catch (SQLException e) {
                clients.put(s.client(), new Client(null, "\"error\":" + json(code(e)) + ",\"connect\":true"));
            }
        }

        for (int i = 0; i < steps.size(); i++) {
            Step s = steps.get(i);
            String step = "{\"step\":" + (i + 1) + ",";
            Client c = clients.get(s.client());
            if (c.conn() == null) {
                emit(step + c.error() + "}");
                continue;
            }
            try {
                emit(step + "\"rows\":" + run(c.conn(), s) + "}");
            } catch (SQLException e) {
                String code = code(e);
                emit(step + "\"error\":" + json(code) + "}");
                if (code.equals("TIMEOUT")) {
                    // pgjdbc closes a connection whose socket timed out.
                    clients.put(s.client(), new Client(null, "\"error\":\"CLOSED\""));
                }
            }
        }

        for (Client c : clients.values()) {
            if (c.conn() != null) {
                try {
                    c.conn().close();
                } catch (SQLException e) {
                    // Closing is best effort; the result is already printed.
                }
            }
        }
        emit("{\"connections\":" + attempts + "}");
        System.exit(0);
    }

    static String run(Connection conn, Step s) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(s.sql())) {
            for (int j = 0; j < s.params().length; j++) {
                ps.setInt(j + 1, s.params()[j]);
            }
            StringBuilder rows = new StringBuilder("[");
            if (ps.execute()) {
                try (ResultSet rs = ps.getResultSet()) {
                    int columns = rs.getMetaData().getColumnCount();
                    while (rs.next()) {
                        if (rows.length() > 1) {
                            rows.append(',');
                        }
                        rows.append('[');
                        for (int k = 1; k <= columns; k++) {
                            if (k > 1) {
                                rows.append(',');
                            }
                            String v = rs.getString(k);
                            rows.append(v == null ? "null" : json(v));
                        }
                        rows.append(']');
                    }
                }
            }
            return rows.append(']').toString();
        }
    }

    /** The SQLSTATE the server sent, a timeout, or DRIVER:<class> for pgjdbc's own errors. */
    static String code(SQLException e) {
        for (Throwable t = e; t != null; t = t.getCause()) {
            if (t instanceof SocketTimeoutException) {
                return "TIMEOUT";
            }
        }
        if (e instanceof PSQLException p && p.getServerErrorMessage() != null && p.getSQLState() != null) {
            return p.getSQLState();
        }
        // pgjdbc gives some of its own errors a SQLSTATE (08006 for a lost connection);
        // it's kept, but marked as the driver's.
        String state = e.getSQLState();
        return "DRIVER:" + e.getClass().getSimpleName() + (state == null ? "" : ":" + state);
    }

    static List<Step> readSteps() throws Exception {
        List<Step> steps = new ArrayList<>();
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        for (String line; (line = in.readLine()) != null; ) {
            String[] parts = line.split("\t", -1);
            if (parts.length != 3) {
                throw new IllegalArgumentException("bad step line: " + line);
            }
            int[] params = parts[2].isEmpty()
                    ? new int[0]
                    : java.util.Arrays.stream(parts[2].split(",")).mapToInt(Integer::parseInt).toArray();
            // JDBC's placeholder is ?; the case SQL has no ? of its own.
            steps.add(new Step(parts[0], parts[1].replaceAll("\\$\\d+", "?"), params));
        }
        return steps;
    }

    static void emit(String line) {
        System.out.println(line);
        System.out.flush();
    }

    static String json(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (char ch : s.toCharArray()) {
            switch (ch) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                default -> {
                    if (ch < 0x20) {
                        b.append(String.format("\\u%04x", (int) ch));
                    } else {
                        b.append(ch);
                    }
                }
            }
        }
        return b.append('"').toString();
    }
}
