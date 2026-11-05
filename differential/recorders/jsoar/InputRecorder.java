package recording;

import java.io.FileWriter;
import java.io.IOException;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Set;

import org.jsoar.kernel.Agent;
import org.jsoar.kernel.memory.Wme;
import org.jsoar.kernel.symbols.Identifier;
import org.jsoar.kernel.symbols.Symbol;

/**
 * Records what a JSoar environment puts on the input link, one decision at a time, in the format
 * Rusty-Soar replays with {@code rusty-soar-cli trace <agent> --input <recording>}.
 *
 * <p>Call {@link #command} with each command the environment gives the interpreter between
 * decisions, and {@link #recordDecision} just before each {@code runFor(1, RunType.DECISIONS)}.
 * Each decision copies the whole input link; on replay, values that didn't change change nothing,
 * as with {@code InputWme.update}.
 */
public final class InputRecorder implements AutoCloseable
{
    private final PrintWriter out;
    private final List<String> commands = new ArrayList<>();
    private int decision = 0;

    public InputRecorder(String path) throws IOException
    {
        out = new PrintWriter(new FileWriter(path));
        out.println("# Rusty-Soar input recording");
    }

    /** A command given to the interpreter since the last decision, such as "rl --set learning on". */
    public synchronized void command(String command)
    {
        commands.add(command);
    }

    /** Records the input for the decision about to run. */
    public synchronized void recordDecision(Agent agent)
    {
        decision++;
        out.println("decision " + decision);
        for (String command : commands)
        {
            out.println("command " + command);
        }
        commands.clear();
        write(agent.getInputOutput().getInputLink(), "", new HashSet<>());
        out.flush();
    }

    /** Writes the WMEs under an identifier, oldest first, so a replay makes them in the same order. */
    private void write(Identifier id, String path, Set<Identifier> seen)
    {
        if (!seen.add(id))
        {
            return;
        }
        List<Wme> wmes = new ArrayList<>();
        for (Iterator<Wme> it = id.getWmes(); it.hasNext();)
        {
            wmes.add(it.next());
        }
        wmes.sort((a, b) -> Integer.compare(a.getTimetag(), b.getTimetag()));
        for (Wme wme : wmes)
        {
            String child = path + wme.getAttribute().toString();
            Symbol value = wme.getValue();
            if (value.asIdentifier() != null)
            {
                write(value.asIdentifier(), child + ".", seen);
            }
            else
            {
                out.println(child + " " + value(value));
            }
        }
    }

    private static String value(Symbol value)
    {
        if (value.asInteger() != null)
        {
            return Long.toString(value.asInteger().getValue());
        }
        if (value.asDouble() != null)
        {
            return Double.toString(value.asDouble().getValue());
        }
        return "|" + value.asString().getValue().replace("\\", "\\\\").replace("|", "\\|") + "|";
    }

    @Override
    public synchronized void close()
    {
        out.close();
    }
}
