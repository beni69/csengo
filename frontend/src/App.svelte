<script lang="ts">
    import toast, { Toaster } from "svelte-french-toast";
    import { z } from "zod";

    const dateZ = z
            .preprocess(arg => {
                if (typeof arg == "string" || arg instanceof Date)
                    return new Date(arg);
                return null;
            }, z.date())
            .refine(d => d > new Date(), "date must be in the future"),
        Form = z.object({
            name: z.string().min(1),
            file_name: z
                .string()
                .min(1)
                .refine(str => {
                    if (str === "$NEW" && !form.files?.length) return false;
                    return true;
                }, "new file missing")
                .transform(s => (s === "$NEW" ? form.files![0].name : s)),
            time: dateZ
                .nullable()
                .or(
                    dateZ
                        .array()
                        .nonempty()
                        .transform(d =>
                            d.map(dd => /T(.+)Z/.exec(dd.toISOString())?.at(-1))
                        )
                        .refine(t => t.map(tt => tt || null) || null)
                )
                .refine(d => {
                    if (d || schedule === "now") return true;
                }, "L"),
        });

    let form = {
        name: "",
        file_name: "",
        time: null as Date | null,
        times: [null] as Array<Date | null>,
        files: null as FileList | null,
    };
    let sending = false;
    const submit = async () => {
        const time =
            schedule === "recurring"
                ? form.times
                : schedule === "scheduled"
                ? form.time
                : null;
        const parse = Form.safeParse({ ...form, time });
        if (!parse.success) return void alert(JSON.stringify(parse.error));

        if (sending) return void toast.error("Already sending");
        sending = true;

        let f;
        if (form.files?.length) {
            const t = toast.loading("Encoding file...");
            f = await form.files?.[0]?.arrayBuffer().then(arrayBufferToBase64);
            console.debug(`file size: ${f.length}`);
            toast.dismiss(t);
        }

        const body = { task: { ...parse.data, type: schedule }, file: f };
        console.debug(body);
        console.debug(parse.data.time, form.time);

        await fetchToast("/api", {
            method: "POST",
            headers: {
                "content-type": "application/json",
            },
            body: JSON.stringify(body),
        });
        sending = false;
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

    const del = async (name: string) =>
            fetchToast(`/api/task/${name}`, {
                method: "DELETE",
            }),
        stop = async () =>
            fetchToast("/api/stop", {
                method: "POST",
            });

    let status: Promise<{
        tasks: {
            name: string;
            file_name: string;
            time: any;
            type: typeof schedule;
        }[];
        files: string[];
        playing: { name: string };
    }>;
    const fetchData = async (loading?: boolean) => {
            const p = fetch("/api/status").then(r => r.json());
            if (loading) {
                status = p;
                await status;
            } else {
                status = Promise.resolve(await p);
            }
        },
        fetchRealtime = async () => {
            while (true) {
                const res = await fetch("/api/status/realtime").then(r =>
                    r.json()
                );
                console.debug("received realtime update:", res);
                status = Promise.resolve({
                    ...(await status),
                    playing: res,
                });
            }
        },
        fetchToast = async (...fetchArgs: Parameters<typeof window.fetch>) => {
            const t = toast.loading("Sending request...");
            let res = await fetch(...fetchArgs);
            toast.dismiss(t);
            if (res.ok) toast.success(await res.text());
            else {
                toast.error("ERROR\n" + res.statusText);
                console.error(await res.text());
            }
            await fetchData();
        };
    fetchData(true).then(fetchRealtime);
    status!.then(console.debug).catch(console.error);

    let schedule: "now" | "scheduled" | "recurring";
</script>

<Toaster />

<main>
    <section class="card">
        <h1>Now playing</h1>
        <div>
            {#await status}
                <p>Loading...</p>
            {:then data}
                {#if data?.playing?.name}
                    <p>{data.playing.name}</p>
                    <button class="btn stop" on:click={stop}
                        >STOP ALL SOUNDS</button>
                {:else}
                    <p>Nothing's playing</p>
                {/if}
            {/await}
        </div>
    </section>
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
                    {#await status}
                        <option disabled>Loading...</option>
                    {:then data}
                        <option value="$NEW">Új file</option>
                        {#each data.files as item}
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
                        <input
                            type="datetime-local"
                            name="time"
                            bind:value={form.time} />
                    {:else if schedule === "recurring"}
                        <button
                            class="add-btn"
                            on:click|preventDefault={() =>
                                (form.times = [...form.times, null])}>
                            +
                        </button>
                        {#each form.times as _, i}
                            <input
                                type="datetime-local"
                                name="time"
                                bind:value={form.times[i]} />
                        {/each}
                    {/if}
                </div>
            </label>

            <input type="submit" value="Go" class="btn go" />
        </form>
        <button class="btn stop" on:click={stop}>STOP ALL SOUNDS</button>
        <p>{JSON.stringify({ ...form, schedule })}</p>
    </section>

    <section class="card">
        <h1>Következő csengetések</h1>
        {#await status}
            <p>loading</p>
        {:then data}
            <div class="grid">
                {#each data.tasks as item}
                    <div class="task">
                        <button class="delete" on:click={() => del(item.name)}>
                            X
                        </button>
                        <p>{item.name}</p>
                        {#if item.type === "recurring"}
                            <p>{item.time.join(", ")}</p>
                        {:else}
                            <p>{new Date(item.time).toISOString()}</p>
                        {/if}
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
        min-height: 100vh;

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
    }

    main {
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
    :global(.grad-bg) {
        backdrop-filter: saturate(180%) blur(10px);
        background-color: rgba(255, 255, 255, 0.4);
    }

    form {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    input,
    .btn {
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

    .btn.go {
        background-image: linear-gradient(
            to right,
            #00d2ff 0%,
            #3a7bd5 51%,
            #00d2ff 100%
        );
    }

    .btn.stop {
        background-image: linear-gradient(
            to right,
            #e52d27 0%,
            #b31217 51%,
            #e52d27 100%
        );
        border: 2px solid #b31217;
        font-size: 0.7em;
        padding: 1em;
    }

    .grid {
        display: grid;
    }
    .task {
        place-self: center;
        position: relative;
        padding: 1rem;
        margin: 0.5em;
        box-shadow: 1px 1px 5px 1px #40618da0;
        transition: 0.1s linear box-shadow;
        border-radius: 8px;
    }
    .task:hover {
        box-shadow: 1px 1px 5px 1px #40618d;
    }
    .delete {
        color: red;
        background: none;
        border: none;

        position: absolute;
        top: 1rem;
        right: 1rem;
        padding: 0.2rem;
        transition: 0.1s linear text-shadow;
    }
    .delete:hover {
        text-shadow: 0 0 3px red;
    }

    .delete:active {
        color: crimson;
    }
</style>
