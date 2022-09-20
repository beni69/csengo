<script lang="ts">
    import { z } from "zod";

    const dateZ = z.preprocess(arg => {
            if (typeof arg == "string" || arg instanceof Date)
                return new Date(arg);
            return null;
        }, z.date()),
        Form = z.object({
            name: z.string().min(1),
            file_name: z
                .string()
                .min(1)
                .refine(str => {
                    if (str === "$NEW" && !form.files?.length) return false;
                    return true;
                }, "new file missing"),
            time: dateZ
                .nullable()
                .or(dateZ.array().nonempty())
                .refine(d => {
                    if (d || schedule === "now") return true;
                }, "L"),
        });

    type _ = z.infer<typeof Form>;

    let form = {
        name: "",
        file_name: "",
        time: null as Date | null,
        times: [null] as Array<Date | null>,
        files: null as FileList | null,
    };
    const submit = async () => {
        const time = schedule === "recurring" ? form.times : form.time;
        const parse = Form.safeParse({ ...form, time });
        if (!parse.success) return void alert(JSON.stringify(parse.error));

        const f = await form.files?.[0]
            ?.arrayBuffer()
            .then(arrayBufferToBase64);
        f && console.debug(`file size: ${f.length}`);

        const body = { task: { ...parse.data, type: "Now" }, file: f };
        console.debug(body);

        fetch("/api", {
            method: "POST",
            headers: {
                "content-type": "application/json",
            },
            body: JSON.stringify(body),
        });
    };
    function arrayBufferToBase64(buffer: ArrayBuffer) {
        var binary = "";
        var bytes = new Uint8Array(buffer);
        var len = bytes.byteLength;
        for (var i = 0; i < len; i++) {
            binary += String.fromCharCode(bytes[i]);
        }
        return window.btoa(binary);
    }

    const fetchData = (): [
        Promise<{ name: string; file_name: string; time: string | null }[]>,
        Promise<string[]>
    ] => [
        fetch("/api/tasks").then(r => r.json()),
        fetch("/api/files").then(r => r.json()),
    ];

    const [tasks, files] = fetchData();
    tasks.then(console.debug).catch(console.error);

    let schedule: "now" | "scheduled" | "recurring";
</script>

<main>
    <section class="card">
        <h1>Új csengetés</h1>
        <form on:submit|preventDefault={submit}>
            <input type="text" bind:value={form.name} placeholder="Név" />

            <label>
                File:
                <select
                    name="file_name"
                    id="file_name"
                    bind:value={form.file_name}>
                    {#await files}
                        <option disabled>Loading...</option>
                    {:then data}
                        <option value="$NEW">Új file</option>
                        {#each data as item}
                            <option value={item}>{item}</option>
                        {/each}
                    {/await}
                </select>

                {#if form.file_name === "$NEW"}
                    <div>
                        <input
                            type="file"
                            name="file_blob"
                            id="file_blob"
                            bind:files={form.files} />
                        <!-- <input
                            type="text"
                            name="file_url"
                            id="file_url"
                            placeholder="File URL" /> -->
                    </div>
                {/if}
            </label>

            <label>
                Mikor?
                <select name="time" id="time" bind:value={schedule}>
                    <option value="now">Most</option>
                    <option value="scheduled">Időzítve, egyszer</option>
                    <option value="recurring">Időzítve, ismétlődően</option>
                </select>

                <div>
                    {#if schedule === "scheduled"}
                        <!--  -->
                        <input
                            type="datetime-local"
                            name="time"
                            bind:value={form.time} />
                    {:else if schedule === "recurring"}
                        <div
                            class="add-btn"
                            on:click={() =>
                                (form.times = [...form.times, null])}>
                            +
                        </div>
                        {#each form.times as _, i}
                            <input
                                type="datetime-local"
                                name="time"
                                bind:value={form.times[i]} />
                        {/each}
                    {/if}
                </div>
            </label>

            <input type="submit" value="Go" class="btn" />
        </form>
        <p>{JSON.stringify({ ...form, schedule })}</p>
    </section>
    <section class="card">
        <h1>Következő csengetések</h1>
        {#await tasks}
            <p>loading</p>
        {:then data}
            <div class="grid">
                {#each data as item}
                    <div class="task">
                        <!-- <p>{JSON.stringify(item)}</p> -->
                        <p>{item.name}</p>
                        <p>{item.time && new Date(item.time).toISOString()}</p>
                    </div>
                {/each}
            </div>
        {:catch}
            <p>fetch failed</p>
        {/await}
    </section>
</main>

<style>
    :global(:root) {
        width: 100vw;
        height: 100vh;
        overflow: hidden;

        font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
        font-size: 16px;
        line-height: 24px;
        font-weight: 400;
        text-align: center;

        color-scheme: light dark;
        background: rgba(9, 9, 121, 1);
        background: linear-gradient(
            315deg,
            rgba(2, 0, 36, 1) 0%,
            rgba(9, 9, 121, 1) 35%,
            rgba(0, 212, 255, 1) 100%
        );

        display: flex;
        align-items: center;
        justify-content: space-evenly;
        flex-direction: column;
    }

    .card {
        backdrop-filter: saturate(180%) blur(10px);
        background-color: rgba(255, 255, 255, 0.4);

        border-radius: 8px;

        padding: 10px;
        margin: 2rem;

        min-width: 33vw;
        max-width: calc(100vw - 50px);
    }

    form {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    input {
        border: 2px solid rgb(0, 90, 255);
        border-radius: 8px;
        padding: 0.3rem;
        margin: 5px;
    }
    input[type="text"] {
        background: #121212;
        color: #eee;
    }

    .btn {
        background-image: linear-gradient(
            to right,
            #00d2ff 0%,
            #3a7bd5 51%,
            #00d2ff 100%
        );
        padding: 15px 45px;
        text-align: center;
        text-transform: uppercase;
        transition: 0.5s;
        background-size: 200% auto;
        color: white;
    }
    .btn:hover {
        background-position: right center; /* change the direction of the change here */
        color: #fff;
        text-decoration: none;
        box-shadow: 0 0 20px #eee;
    }

    .grid {
        display: grid;
    }
    .task {
        place-self: stretch;
    }
</style>
